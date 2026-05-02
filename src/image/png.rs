//! Minimal in-tree PNG writer (RGBA8 only, uncompressed deflate).
//!
//! Hand-rolled to keep the generator dep tree at `oxideav-core` +
//! `thiserror`. The output is a fully-spec-compliant PNG that the
//! `oxideav-png` decoder (and every other PNG decoder we tested) reads
//! back without complaint:
//!
//! - 8-byte PNG signature
//! - IHDR (8-bit truecolor + alpha, no interlace, no filter shift)
//! - IDAT carrying a zlib stream of stored (uncompressed) deflate
//!   blocks; each scanline is filter-byte 0 + raw row bytes
//! - IEND
//!
//! Compression is a non-goal — the bytes go straight into a memory-
//! backed reader and the PNG demuxer decodes them once.

use super::Rgba8Image;

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// Encode an RGBA8 image as a standalone PNG byte stream.
pub fn encode(img: &Rgba8Image) -> Vec<u8> {
    let mut out = Vec::with_capacity(img.pixels.len() + 256);
    out.extend_from_slice(&PNG_MAGIC);

    // IHDR — 13 bytes payload.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.width.to_be_bytes());
    ihdr.extend_from_slice(&img.height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // colour type: truecolor + alpha
    ihdr.push(0); // compression: deflate
    ihdr.push(0); // filter: PNG default
    ihdr.push(0); // interlace: none
    write_chunk(&mut out, b"IHDR", &ihdr);

    // IDAT — filtered scanlines wrapped in a zlib stream.
    let raw = filter_scanlines_none(img);
    let zlib = zlib_wrap_uncompressed(&raw);
    write_chunk(&mut out, b"IDAT", &zlib);

    // IEND — empty.
    write_chunk(&mut out, b"IEND", &[]);
    out
}

/// PNG filter type 0 ("None") prepended to every scanline.
fn filter_scanlines_none(img: &Rgba8Image) -> Vec<u8> {
    let row_bytes = (img.width as usize) * 4;
    let mut out = Vec::with_capacity((row_bytes + 1) * img.height as usize);
    for y in 0..img.height as usize {
        out.push(0); // filter type
        let start = y * row_bytes;
        out.extend_from_slice(&img.pixels[start..start + row_bytes]);
    }
    out
}

/// Wrap raw bytes in a zlib stream made of stored (uncompressed) deflate
/// blocks. Spec: each block = `BFINAL:1 | BTYPE:00 | LEN:16 | NLEN:16
/// | LITERAL_BYTES`. LEN is the per-block byte count (≤ 65535).
fn zlib_wrap_uncompressed(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    // zlib header: CMF=0x78, FLG chosen so (CMF*256 + FLG) % 31 == 0 and
    // FDICT=0, FLEVEL=0 (fastest). 0x78 0x01 is the canonical "no compression"
    // header used by zlib's `Z_NO_COMPRESSION` mode.
    out.push(0x78);
    out.push(0x01);

    let mut pos = 0;
    while pos < data.len() {
        let chunk_len = (data.len() - pos).min(0xFFFF);
        let last_block = pos + chunk_len == data.len();
        // First byte: BFINAL in bit 0, BTYPE=00 in bits 1-2, rest 0.
        out.push(if last_block { 0x01 } else { 0x00 });
        out.extend_from_slice(&(chunk_len as u16).to_le_bytes());
        out.extend_from_slice(&(!chunk_len as u16).to_le_bytes());
        out.extend_from_slice(&data[pos..pos + chunk_len]);
        pos += chunk_len;
    }
    // Empty `data` still produces a single empty stored block.
    if data.is_empty() {
        out.push(0x01);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    }
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

/// Adler-32 checksum (RFC 1950).
fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Write a single PNG chunk: length(4 BE) | type(4) | data | CRC32(4 BE)
/// of (type | data).
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// CRC-32/ISO-HDLC (the variant PNG uses, identical to zlib/POSIX).
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        let mut c = (crc ^ byte as u32) & 0xFF;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB88320
            } else {
                c >> 1
            };
        }
        crc = (crc >> 8) ^ c;
    }
    crc ^ 0xFFFFFFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_signature_and_chunks() {
        let img = Rgba8Image::new(2, 2);
        let bytes = encode(&img);
        assert_eq!(&bytes[0..8], &PNG_MAGIC);
        // First chunk type after signature is IHDR.
        assert_eq!(&bytes[12..16], b"IHDR");
    }

    #[test]
    fn crc32_matches_known_vector() {
        // Standard vector: CRC32("123456789") = 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn adler32_matches_known_vector() {
        // Standard vector: Adler32("Wikipedia") = 0x11E60398.
        assert_eq!(adler32(b"Wikipedia"), 0x11E60398);
    }

    #[test]
    fn small_image_decodes_via_oxideav_png_round_trip_shape() {
        // We don't pull oxideav-png into the dep tree just for tests,
        // but we can structurally verify the PNG: signature + IHDR + IDAT
        // + IEND in order.
        let img = Rgba8Image {
            width: 1,
            height: 1,
            pixels: vec![255, 0, 0, 255],
        };
        let bytes = encode(&img);
        let pos_ihdr = find(&bytes, b"IHDR").unwrap();
        let pos_idat = find(&bytes, b"IDAT").unwrap();
        let pos_iend = find(&bytes, b"IEND").unwrap();
        assert!(pos_ihdr < pos_idat);
        assert!(pos_idat < pos_iend);
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }
}
