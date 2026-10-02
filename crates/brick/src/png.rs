//! PNG output, for screenshots of the UI taken by scripted runs.

use std::io::Write;

/// Write tightly packed RGB as a PNG, for screenshots.
///
/// Uncompressed ("stored") deflate blocks: a valid PNG with no compression
/// library linked in, which is all a debugging aid needs.
pub fn write_png(path: &str, w: usize, h: usize, rgb: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for row in rgb.chunks_exact(w * 3) {
        raw.push(0); // filter type: none
        raw.extend_from_slice(row);
    }

    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(0xffff).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(blocks.peek().is_none() as u8);
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut header = Vec::new();
    header.extend_from_slice(&(w as u32).to_be_bytes());
    header.extend_from_slice(&(h as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB

    let mut file = std::fs::File::create(path)?;
    file.write_all(b"\x89PNG\r\n\x1a\n")?;
    for (tag, data) in [(b"IHDR", &header), (b"IDAT", &zlib), (b"IEND", &Vec::new())] {
        file.write_all(&(data.len() as u32).to_be_bytes())?;
        file.write_all(tag)?;
        file.write_all(data)?;
        file.write_all(&crc32(tag, data).to_be_bytes())?;
    }
    Ok(())
}

fn crc32(tag: &[u8], data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in tag.iter().chain(data) {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_png_specification() {
        // The CRC of an empty IEND chunk is fixed and appears in every PNG.
        assert_eq!(crc32(b"IEND", &[]), 0xae42_6082);
    }
}
