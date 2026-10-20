fn idat(png: &[u8]) -> Vec<u8> {
    let mut pos = 8usize; let mut out = Vec::new();
    while pos + 8 <= png.len() {
        let len = u32::from_be_bytes(png[pos..pos+4].try_into().unwrap()) as usize;
        if &png[pos+4..pos+8] == b"IDAT".as_slice() { out.extend_from_slice(&png[pos+8..pos+8+len]); }
        if &png[pos+4..pos+8] == b"IEND".as_slice() { break; }
        pos += 12 + len;
    }
    out
}
fn main() {
    for p in std::env::args().skip(1) {
        let png = std::fs::read(&p).unwrap();
        let z = idat(&png);
        match gb_tests::debug_inflate_len(&z) {
            Some(n) => println!("{p}: inflate ok {n}"),
            None => println!("{p}: inflate None"),
        }
    }
}
