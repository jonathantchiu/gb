//! Minimal PNG decoding for the acid2 reference images, with no external
//! crate: zlib/DEFLATE (stored + fixed + dynamic Huffman) inflate, PNG
//! scanline un-filtering, and mapping to the canonical byte-per-pixel
//! greyscale space the framebuffer oracle compares in.

use std::path::Path;

/// Decode a greyscale reference PNG to our canonical space: one byte per
/// pixel, row-major, where PNG sample 0 (black) -> 0x00 .. 3 (white) -> 0xFF.
/// This matches the PPU convention that color 0 (BGP color 0) is white = 0xFF.
pub fn decode_reference_png(path: &Path) -> Vec<u8> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(&data[0..8], b"\x89PNG\r\n\x1a\n", "not a png");
    let mut pos = 8;
    let (mut w, mut h, mut bit_depth, mut color_type) = (0u32, 0u32, 0u8, 0u8);
    let mut idat = Vec::new();
    while pos < data.len() {
        let len = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        let ctype = &data[pos + 4..pos + 8];
        let cdata = &data[pos + 8..pos + 8 + len];
        match ctype {
            b"IHDR" => {
                w = u32::from_be_bytes(cdata[0..4].try_into().unwrap());
                h = u32::from_be_bytes(cdata[4..8].try_into().unwrap());
                bit_depth = cdata[8];
                color_type = cdata[9];
                assert_eq!(cdata[12], 0, "interlaced reference not supported");
            }
            b"IDAT" => idat.extend_from_slice(cdata),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    let raw = inflate(&idat);
    unfilter(&raw, w as usize, h as usize, bit_depth, color_type)
}

fn unfilter(raw: &[u8], w: usize, h: usize, bd: u8, ct: u8) -> Vec<u8> {
    let (row_bytes, bpp) = match (bd, ct) {
        (2, 0) => (w.div_ceil(4), 1),
        (8, 0) => (w, 1),
        (8, 2) => (w * 3, 3),
        (8, 6) => (w * 4, 4),
        _ => panic!("unsupported reference png: bit_depth={bd} color_type={ct}"),
    };
    let stride = row_bytes + 1;
    assert_eq!(raw.len(), stride * h, "raw size");
    let mut out = vec![0u8; w * h];
    let mut prev = vec![0u8; row_bytes];
    for y in 0..h {
        let filter = raw[y * stride];
        let mut cur = raw[y * stride + 1..(y + 1) * stride].to_vec();
        for i in 0..row_bytes {
            let a = if i >= bpp { cur[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let recon = match filter {
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) >> 1) as u8,
                4 => {
                    let p = a as i32 + b as i32 - c as i32;
                    let (pa, pb, pc) =
                        ((p - a as i32).abs(), (p - b as i32).abs(), (p - c as i32).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                _ => 0,
            };
            cur[i] = cur[i].wrapping_add(recon);
        }
        prev = cur.clone();
        for x in 0..w {
            out[y * w + x] = match (bd, ct) {
                (2, 0) => {
                    // PNG greyscale 0=black .. 3=white; pinned DMG mapping.
                    let byte = cur[x / 4];
                    let g = (byte >> (6 - (x % 4) * 2)) & 0x3;
                    [0x00, 0x55, 0xAA, 0xFF][g as usize]
                }
                (8, 0) => cur[x],
                (8, 2) => cur[x * 3],
                (8, 6) => cur[x * 4],
                _ => unreachable!(),
            };
        }
    }
    out
}

// --- zlib / DEFLATE inflate ---

fn inflate(data: &[u8]) -> Vec<u8> {
    assert!(data.len() > 6, "zlib too short");
    // skip 2-byte zlib header; adler32 at the end is not checked
    let mut bits = BitReader { data: &data[2..], pos: 0 };
    let mut out = Vec::new();
    loop {
        let bfinal = bits.get(1) != 0;
        match bits.get(2) {
            0 => {
                bits.align();
                let len = bits.get(16) as usize;
                let _nlen = bits.get(16);
                for _ in 0..len {
                    out.push(bits.get(8) as u8);
                }
            }
            1 => {
                let (lit, dist) = fixed_huffs();
                inflate_block(&mut bits, &mut out, &lit, &dist);
            }
            2 => {
                let (lit, dist) = read_dynamic(&mut bits);
                inflate_block(&mut bits, &mut out, &lit, &dist);
            }
            _ => panic!("reserved btype"),
        }
        if bfinal {
            break;
        }
    }
    out
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    fn get(&mut self, n: usize) -> u32 {
        let mut v = 0u32;
        for i in 0..n {
            let byte = self.data[self.pos / 8];
            let bit = (byte >> (self.pos % 8)) & 1;
            v |= (bit as u32) << i;
            self.pos += 1;
        }
        v
    }
    fn align(&mut self) {
        self.pos = (self.pos + 7) & !7;
    }
}

struct Huff {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huff {
    fn new(lengths: &[u8]) -> Huff {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l != 0 {
                counts[l as usize] += 1;
            }
        }
        let mut offs = [0u16; 16];
        for i in 1..15 {
            offs[i + 1] = offs[i] + counts[i];
        }
        let mut symbols = vec![0u16; lengths.iter().filter(|&&l| l != 0).count()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Huff { counts, symbols }
    }
    fn decode(&self, bits: &mut BitReader) -> u16 {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.get(1) as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return self.symbols[(index + (code - first)) as usize];
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        panic!("bad huffman code");
    }
}

fn fixed_huffs() -> (Huff, Huff) {
    let mut lit = [0u8; 288];
    for (i, l) in lit.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    (Huff::new(&lit), Huff::new(&[5u8; 30]))
}

fn read_dynamic(bits: &mut BitReader) -> (Huff, Huff) {
    let hlit = bits.get(5) as usize + 257;
    let hdist = bits.get(5) as usize + 1;
    let hclen = bits.get(4) as usize + 4;
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut cl = [0u8; 19];
    for i in 0..hclen {
        cl[ORDER[i]] = bits.get(3) as u8;
    }
    let cl_huff = Huff::new(&cl);
    let mut lengths = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < hlit + hdist {
        match cl_huff.decode(bits) {
            sym @ 0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                let rep = 3 + bits.get(2);
                let v = lengths[i - 1];
                for _ in 0..rep {
                    lengths[i] = v;
                    i += 1;
                }
            }
            17 => i += 3 + bits.get(3) as usize,
            18 => i += 11 + bits.get(7) as usize,
            _ => panic!(),
        }
    }
    (Huff::new(&lengths[..hlit]), Huff::new(&lengths[hlit..]))
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn inflate_block(bits: &mut BitReader, out: &mut Vec<u8>, lit: &Huff, dist: &Huff) {
    loop {
        match lit.decode(bits) {
            sym @ 0..=255 => out.push(sym as u8),
            256 => break,
            sym @ 257..=285 => {
                let li = (sym - 257) as usize;
                let len = LEN_BASE[li] as usize + bits.get(LEN_EXTRA[li] as usize) as usize;
                let dsym = dist.decode(bits) as usize;
                let d = DIST_BASE[dsym] as usize + bits.get(DIST_EXTRA[dsym] as usize) as usize;
                for _ in 0..len {
                    let b = out[out.len() - d];
                    out.push(b);
                }
            }
            _ => panic!("bad symbol"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_dmg_acid2_reference() {
        let px = decode_reference_png(Path::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/roms/dmg-acid2/reference-dmg.png"),
        ));
        assert_eq!(px.len(), 160 * 144);
        // top-left 8 pixels are white; the reference has a black border region
        assert_eq!(&px[..8], &[0xFF; 8]);
        assert!(px.iter().any(|&p| p == 0x00));
    }
}
