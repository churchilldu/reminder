//! A minimal ICO writer.
//!
//! Windows Vista and later accept PNG-compressed image entries inside .ico
//! files, so the app icon can be a handful of PNGs (the ones icon.rs already
//! renders) wrapped in an ICO header -- no second pixel format, no dependency.
//! This tool targets Windows 10/11, where that is all the shell ever asks for.

/// Encode a .ico file from (size, PNG bytes) entries.
pub fn encode(entries: &[(u32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();

    // ICONDIR
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // 1 = icon
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    // ICONDIRENTRY x N. Image data begins right after the last entry.
    let mut offset = 6 + entries.len() * 16;
    for &(size, png) in entries {
        let dim = if size >= 256 { 0 } else { size as u8 }; // 0 means 256
        out.push(dim);
        out.push(dim);
        out.push(0); // no palette
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += png.len();
    }

    for &(_, png) in entries {
        out.extend_from_slice(png);
    }

    out
}
