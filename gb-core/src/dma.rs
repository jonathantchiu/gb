/// OAM DMA (FF46).
///
/// Writing FF46 during M-cycle k: k+1 is a setup cycle, and bytes 0..159 are
/// copied during k+2..k+161, one per M-cycle. OAM is inaccessible to the CPU
/// while bytes are being copied. Writing FF46 while a transfer is running
/// restarts it: the old transfer continues through the new one's setup cycle,
/// so OAM stays blocked throughout.
#[derive(Default)]
pub struct OamDma {
    pub reg: u8,
    /// (source page, M-cycles of setup remaining)
    pending: Option<(u8, u8)>,
    /// (source base address, next byte index)
    active: Option<(u16, u8)>,
    /// Set for an M-cycle in which a byte is copied.
    transfer: Option<Transfer>,
}

#[derive(Clone, Copy)]
pub struct Transfer {
    pub src: u16,
    pub dst: u8,
}

impl OamDma {
    pub fn new() -> Self {
        OamDma {
            reg: 0xFF,
            ..Default::default()
        }
    }

    pub fn start(&mut self, val: u8) {
        self.reg = val;
        self.pending = Some((val, 1));
    }

    /// Advances to the next M-cycle. Returns the byte transfer (if any) that
    /// occupies this M-cycle; the bus performs it.
    pub fn begin_mcycle(&mut self) -> Option<Transfer> {
        self.transfer = self.active.map(|(base, i)| Transfer {
            src: base + i as u16,
            dst: i,
        });
        if let Some((base, i)) = self.active {
            self.active = if i + 1 < 160 { Some((base, i + 1)) } else { None };
        }
        if let Some((page, n)) = self.pending {
            if n <= 1 {
                self.pending = None;
                self.active = Some(((page as u16) << 8, 0));
            } else {
                self.pending = Some((page, n - 1));
            }
        }
        self.transfer
    }

    /// The transfer happening during the current M-cycle, if any.
    pub fn current(&self) -> Option<Transfer> {
        self.transfer
    }
}
