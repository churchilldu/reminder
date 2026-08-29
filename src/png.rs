//! A minimal PNG encoder.
//!
//! Only enough of the format to write RGBA images, so the level icons can be
//! generated without pulling in an image or compression crate. Pixel data is
//! stored in uncompressed DEFLATE blocks: the icons are tiny and written once,
//! so paying a few extra kilobytes to avoid a dependency is a good trade.

/// CRC-32 (IEEE), computed bitwise. No table: the inputs here are a few tens
/// of kilobytes, so the simpler code is worth more than the speed.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

/// Adler-32, the checksum zlib appends to a stream.
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Wrap `data` in a zlib stream of stored (uncompressed) DEFLATE blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    // 0x78 0x01: deflate, 32K window, fastest level. The pair has to be a
    // multiple of 31, and 0x7801 is.
    let mut out = vec![0x78, 0x01];

    if data.is_empty() {
        // A single final, empty stored block.
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        // A stored block's length field is 16 bits, so chunk accordingly.
        let blocks: Vec<&[u8]> = data.chunks(0xFFFF).collect();
        let last = blocks.len() - 1;
        for (i, block) in blocks.iter().enumerate() {
            out.push(if i == last { 1 } else { 0 }); // BFINAL, BTYPE=00
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes()); // NLEN
            out.extend_from_slice(block);
        }
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);

    let mut crc_input = Vec::with_capacity(4 + payload.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(payload);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// Encode 8-bit RGBA pixels (row-major, `width * height * 4` bytes) as a PNG.
pub fn encode_rgba(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let stride = width as usize * 4;
    debug_assert_eq!(pixels.len(), stride * height as usize);

    // Every scanline is prefixed with its filter type; 0 means "none".
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for y in 0..height as usize {
        raw.push(0);
        raw.extend_from_slice(&pixels[y * stride..(y + 1) * stride]);
    }

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[
        8, // bit depth
        6, // colour type 6 = RGBA
        0, // deflate
        0, // adaptive filtering
        0, // no interlacing
    ]);

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    push_chunk(&mut out, b"IEND", &[]);
    out
}
