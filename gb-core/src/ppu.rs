use crate::{SCREEN_H, SCREEN_W};
use std::collections::VecDeque;

pub const DMG_SHADES: [u8; 4] = [0xFF, 0xAA, 0x55, 0x00];

#[derive(Clone, Copy, Default)]
struct ObjPixel {
    color: u8,
    palette1: bool,
    bg_priority: bool,
}

#[derive(Clone, Copy, Default)]
struct SpriteEntry {
    y: u8,
    x: u8,
    tile: u8,
    flags: u8,
    oam_index: u8,
    fetched: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FetcherState {
    GetTile,
    GetDataLow,
    GetDataHigh,
    Push,
}

pub struct Ppu {
    pub lcdc: u8,
    pub stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub wy: u8,
    pub wx: u8,
    pub vram: [u8; 0x2000],
    pub oam: [u8; 0xA0],
    pub framebuffer: Vec<u8>,
    pub frame_count: u64,
    dot: u16,
    mode: u8,
    stat_irq_line: bool,

    // Pixel FIFO & Fetcher state
    bg_fifo: VecDeque<u8>,
    obj_fifo: VecDeque<ObjPixel>,
    fetcher_state: FetcherState,
    fetcher_dots: u8,
    fetcher_x: u8,
    fetch_tile_id: u8,
    fetch_tile_lo: u8,
    fetch_tile_hi: u8,
    sprite_fetch_dots: u8,

    lcd_x: u8,
    scx_discard: u8,
    wy_triggered: bool,
    window_active_on_line: bool,
    window_line: u8,

    scanline_sprites: Vec<SpriteEntry>,
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            lcdc: 0x91,
            stat: 0x85,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            vram: [0; 0x2000],
            oam: [0; 0xA0],
            framebuffer: vec![0xFF; SCREEN_W * SCREEN_H],
            frame_count: 0,
            dot: 0,
            mode: 2,
            stat_irq_line: false,
            bg_fifo: VecDeque::with_capacity(16),
            obj_fifo: VecDeque::with_capacity(16),
            fetcher_state: FetcherState::GetTile,
            fetcher_dots: 0,
            fetcher_x: 0,
            fetch_tile_id: 0,
            fetch_tile_lo: 0,
            fetch_tile_hi: 0,
            sprite_fetch_dots: 0,
            lcd_x: 0,
            scx_discard: 0,
            wy_triggered: false,
            window_active_on_line: false,
            window_line: 0,
            scanline_sprites: Vec::with_capacity(10),
        }
    }

    pub fn mode(&self) -> u8 {
        if self.lcdc & 0x80 == 0 {
            0
        } else {
            self.mode
        }
    }

    pub fn read_vram(&self, offset: u16) -> u8 {
        self.vram[offset as usize]
    }

    pub fn write_vram(&mut self, offset: u16, val: u8) {
        self.vram[offset as usize] = val;
    }

    pub fn read_oam(&self, offset: u16) -> u8 {
        self.oam[offset as usize]
    }

    pub fn write_oam(&mut self, offset: u16, val: u8) {
        self.oam[offset as usize] = val;
    }

    pub fn read_stat(&self) -> u8 {
        let coincidence = if self.ly == self.lyc { 0x04 } else { 0 };
        (self.stat & 0x78) | 0x80 | coincidence | self.mode()
    }

    pub fn write_stat(&mut self, val: u8, if_reg: &mut u8) {
        self.stat = val & 0x78;
        self.update_stat_irq(if_reg);
    }

    pub fn write_lyc(&mut self, val: u8, if_reg: &mut u8) {
        self.lyc = val;
        self.update_stat_irq(if_reg);
    }

    pub fn read_lcdc(&self) -> u8 {
        self.lcdc
    }

    pub fn write_lcdc(&mut self, val: u8, if_reg: &mut u8) {
        let was_on = self.lcdc & 0x80 != 0;
        let now_on = val & 0x80 != 0;
        self.lcdc = val;
        if was_on && !now_on {
            self.ly = 0;
            self.dot = 0;
            self.mode = 0;
            self.stat_irq_line = false;
            self.wy_triggered = false;
            self.window_active_on_line = false;
            self.window_line = 0;
        } else if !was_on && now_on {
            self.ly = 0;
            self.dot = 0;
            self.mode = 2;
            self.wy_triggered = false;
            self.window_active_on_line = false;
            self.window_line = 0;
            self.oam_scan();
            self.update_stat_irq(if_reg);
        }
    }

    pub fn write_ly(&mut self) {}

    fn update_stat_irq(&mut self, if_reg: &mut u8) {
        if self.lcdc & 0x80 == 0 {
            self.stat_irq_line = false;
            return;
        }
        let lyc_match = self.ly == self.lyc && (self.stat & 0x40 != 0);
        let mode_match = match self.mode {
            0 => self.stat & 0x08 != 0,
            1 => self.stat & 0x10 != 0,
            2 => self.stat & 0x20 != 0,
            _ => false,
        };
        let line = lyc_match || mode_match;
        if line && !self.stat_irq_line {
            *if_reg |= 0x02;
        }
        self.stat_irq_line = line;
    }

    fn oam_scan(&mut self) {
        self.scanline_sprites.clear();
        let height: i16 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let ly = self.ly as i16 + 16;
        for i in 0..40u8 {
            let base = (i as usize) * 4;
            let y = self.oam[base];
            let x = self.oam[base + 1];
            let tile = self.oam[base + 2];
            let flags = self.oam[base + 3];
            if ly >= y as i16 && ly < y as i16 + height {
                self.scanline_sprites.push(SpriteEntry {
                    y,
                    x,
                    tile,
                    flags,
                    oam_index: i,
                    fetched: false,
                });
                if self.scanline_sprites.len() == 10 {
                    break;
                }
            }
        }
        // DMG priority: smaller X first, then smaller OAM index
        self.scanline_sprites
            .sort_by_key(|s| (s.x, s.oam_index));
    }

    fn start_mode3(&mut self) {
        // Re-scan OAM at the end of Mode 2 in case register/OAM writes occurred during Mode 2
        self.oam_scan();
        if (self.lcdc & 0x20 != 0) && self.ly == self.wy {
            self.wy_triggered = true;
        }
        self.mode = 3;
        self.bg_fifo.clear();
        self.obj_fifo.clear();
        self.fetcher_state = FetcherState::GetTile;
        self.fetcher_dots = 0;
        self.fetcher_x = 0;
        self.sprite_fetch_dots = 0;
        self.lcd_x = 0;
        self.scx_discard = self.scx & 7;
        self.window_active_on_line = false;
    }

    fn fetch_sprite(&mut self, idx: usize) {
        let sp = self.scanline_sprites[idx];
        let height: u8 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let mut row = (self.ly as i16 + 16 - sp.y as i16) as u8;
        if sp.flags & 0x40 != 0 {
            row = (height - 1) - row;
        }
        let tile = if height == 16 {
            if row < 8 {
                sp.tile & 0xFE
            } else {
                row -= 8;
                sp.tile | 0x01
            }
        } else {
            sp.tile
        };
        let addr = (tile as usize) * 16 + (row as usize) * 2;
        let lo = self.vram[addr];
        let hi = self.vram[addr + 1];
        let flip_x = sp.flags & 0x20 != 0;
        let palette1 = sp.flags & 0x10 != 0;
        let bg_priority = sp.flags & 0x80 != 0;

        // If sp.x < 8, the leftmost (8 - sp.x) pixels are off-screen to the left
        let start_col = if sp.x < 8 { 8 - sp.x } else { 0 };
        let count = (8 - start_col) as usize;
        while self.obj_fifo.len() < count {
            self.obj_fifo.push_back(ObjPixel::default());
        }
        for i in 0..count {
            let col = start_col + i as u8;
            let bit = if flip_x { col } else { 7 - col };
            let c = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1);
            if c != 0 && self.obj_fifo[i].color == 0 {
                self.obj_fifo[i] = ObjPixel {
                    color: c,
                    palette1,
                    bg_priority,
                };
            }
        }
    }

    fn step_fetcher(&mut self) {
        self.fetcher_dots += 1;
        match self.fetcher_state {
            FetcherState::GetTile => {
                if self.fetcher_dots == 2 {
                    self.fetcher_dots = 0;
                    let (map_base, tile_x, tile_y) = if self.window_active_on_line {
                        let base = if self.lcdc & 0x40 != 0 {
                            0x1C00usize
                        } else {
                            0x1800usize
                        };
                        let tx = (self.fetcher_x & 0x1F) as usize;
                        let ty = ((self.window_line / 8) & 0x1F) as usize;
                        (base, tx, ty)
                    } else {
                        let base = if self.lcdc & 0x08 != 0 {
                            0x1C00usize
                        } else {
                            0x1800usize
                        };
                        let tx = ((self.scx / 8).wrapping_add(self.fetcher_x) & 0x1F) as usize;
                        let ty = ((self.ly.wrapping_add(self.scy) / 8) & 0x1F) as usize;
                        (base, tx, ty)
                    };
                    self.fetch_tile_id = self.vram[map_base + tile_y * 32 + tile_x];
                    self.fetcher_state = FetcherState::GetDataLow;
                }
            }
            FetcherState::GetDataLow => {
                if self.fetcher_dots == 2 {
                    self.fetcher_dots = 0;
                    let addr = self.tile_data_addr();
                    self.fetch_tile_lo = self.vram[addr];
                    self.fetcher_state = FetcherState::GetDataHigh;
                }
            }
            FetcherState::GetDataHigh => {
                if self.fetcher_dots == 2 {
                    self.fetcher_dots = 0;
                    let addr = self.tile_data_addr();
                    self.fetch_tile_hi = self.vram[addr + 1];
                    self.fetcher_state = FetcherState::Push;
                    self.try_push_bg_fifo();
                }
            }
            FetcherState::Push => {
                self.try_push_bg_fifo();
            }
        }
    }

    fn tile_data_addr(&self) -> usize {
        let fine_y = if self.window_active_on_line {
            (self.window_line & 7) as usize
        } else {
            (self.ly.wrapping_add(self.scy) & 7) as usize
        };
        let base = if self.lcdc & 0x10 != 0 {
            (self.fetch_tile_id as usize) * 16
        } else {
            (0x1000i32 + (self.fetch_tile_id as i8 as i32) * 16) as usize
        };
        base + fine_y * 2
    }

    fn try_push_bg_fifo(&mut self) {
        if self.bg_fifo.is_empty() {
            for bit in (0..8).rev() {
                let c = (((self.fetch_tile_hi >> bit) & 1) << 1)
                    | ((self.fetch_tile_lo >> bit) & 1);
                self.bg_fifo.push_back(c);
            }
            self.fetcher_x = self.fetcher_x.wrapping_add(1);
            self.fetcher_state = FetcherState::GetTile;
            self.fetcher_dots = 0;
        }
    }

    fn tick_mode3(&mut self, if_reg: &mut u8) {
        // 1. Check Window activation at current lcd_x
        if !self.window_active_on_line
            && self.wy_triggered
            && (self.lcdc & 0x20 != 0)
            && self.wx <= 166
            && (self.lcd_x as u16 + 7 >= self.wx as u16)
        {
            self.window_active_on_line = true;
            self.bg_fifo.clear();
            self.fetcher_state = FetcherState::GetTile;
            self.fetcher_dots = 0;
            self.fetcher_x = 0;
            self.scx_discard = if self.wx < 7 { 7 - self.wx } else { 0 };
        }

        // 2. If currently fetching a sprite, advance sprite fetch stall
        if self.sprite_fetch_dots > 0 {
            self.sprite_fetch_dots -= 1;
            return;
        }

        // 3. Advance BG/Window Fetcher so bg_fifo is populated before sprite check & pixel shift
        self.step_fetcher();

        // 4. Check if any sprite triggers at current lcd_x before shifting pixel at lcd_x
        if self.scx_discard == 0 && !self.bg_fifo.is_empty() && (self.lcdc & 0x02 != 0) {
            for idx in 0..self.scanline_sprites.len() {
                let sp = self.scanline_sprites[idx];
                if !sp.fetched && sp.x > 0 && sp.x <= self.lcd_x + 8 {
                    self.scanline_sprites[idx].fetched = true;
                    self.fetch_sprite(idx);
                    self.sprite_fetch_dots = 5;
                    return;
                }
            }
        }

        // 5. Shift 1 pixel from FIFO if available
        if let Some(raw_bg) = self.bg_fifo.pop_front() {
            if self.scx_discard > 0 {
                self.scx_discard -= 1;
                return;
            }
            let obj = self.obj_fifo.pop_front().unwrap_or_default();
            let bg_color = if self.lcdc & 0x01 != 0 { raw_bg } else { 0 };
            let shade = if (self.lcdc & 0x02 != 0)
                && obj.color != 0
                && (!obj.bg_priority || bg_color == 0)
            {
                let pal = if obj.palette1 { self.obp1 } else { self.obp0 };
                (pal >> (obj.color * 2)) & 0x03
            } else {
                (self.bgp >> (bg_color * 2)) & 0x03
            };
            let fb_idx = (self.ly as usize) * SCREEN_W + (self.lcd_x as usize);
            self.framebuffer[fb_idx] = DMG_SHADES[shade as usize];
            self.lcd_x += 1;
            if self.lcd_x == SCREEN_W as u8 {
                if self.window_active_on_line {
                    self.window_line = self.window_line.wrapping_add(1);
                }
                self.mode = 0;
                self.update_stat_irq(if_reg);
            }
        }
    }

    pub fn tick(&mut self, if_reg: &mut u8) {
        if self.lcdc & 0x80 == 0 {
            self.stat_irq_line = false;
            return;
        }

        self.dot += 1;

        if self.ly < 144 {
            if self.dot == 80 {
                self.start_mode3();
                self.update_stat_irq(if_reg);
            } else if self.mode == 3 {
                self.tick_mode3(if_reg);
            }
        }

        if self.dot == 456 {
            self.dot = 0;
            self.ly += 1;
            if self.ly == 144 {
                self.mode = 1;
                *if_reg |= 0x01;
                self.frame_count += 1;
            } else if self.ly == 154 {
                self.ly = 0;
                self.mode = 2;
                self.wy_triggered = false;
                self.window_line = 0;
                if (self.lcdc & 0x20 != 0) && self.wy == 0 {
                    self.wy_triggered = true;
                }
            } else if self.ly < 144 {
                self.mode = 2;
                if (self.lcdc & 0x20 != 0) && self.ly == self.wy {
                    self.wy_triggered = true;
                }
            }
            self.update_stat_irq(if_reg);
        }
    }
}
