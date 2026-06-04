//! LZMA decompression for NSIS data blocks.
//!
//! NSIS LZMA streams begin with a properties byte (typically `0x5D` for
//! lc=3, lp=0, pb=2) followed by a 4-byte little-endian dictionary size.

use crate::error::Error;

/// Decompresses an NSIS LZMA stream.
///
/// NSIS LZMA streams use the raw LZMA format: a properties byte followed
/// by a 4-byte dictionary size, then the compressed data. The uncompressed
/// size is not stored in the NSIS LZMA header.
///
/// # Arguments
///
/// - `compressed`: the raw LZMA stream (properties + dict size + data)
/// - `max_output`: maximum decompressed size
/// - `expected_size`: if `Some`, the exact expected decompressed size is
///   written into the LZMA header so `lzma-rs` stops after that many bytes.
///   If `None`, the size is set to unknown and we rely on the EOS marker.
///
/// # Errors
///
/// Returns [`Error::DecompressionFailed`] if the LZMA stream is invalid.
pub fn decompress_lzma(
    compressed: &[u8],
    max_output: usize,
    expected_size: Option<usize>,
) -> Result<Vec<u8>, Error> {
    if compressed.len() < 5 {
        return Err(Error::DecompressionFailed {
            method: "lzma",
            detail: "LZMA stream too short (need at least 5 bytes for header)".into(),
        });
    }

    // Build a standard LZMA header for lzma-rs:
    // Bytes 0:   properties byte (from NSIS stream)
    // Bytes 1-4: dictionary size (from NSIS stream)
    // Bytes 5-12: uncompressed size (known or 0xFFFFFFFFFFFFFFFF for unknown)
    let uncompressed_size_bytes: [u8; 8] = match expected_size {
        Some(size) => (size as u64).to_le_bytes(),
        None => [0xFF; 8],
    };

    let mut lzma_header = Vec::with_capacity(compressed.len().saturating_add(8));
    let (props, body) = compressed.split_at(5);
    lzma_header.extend_from_slice(props); // props + dict_size
    lzma_header.extend_from_slice(&uncompressed_size_bytes);
    lzma_header.extend_from_slice(body);

    let mut output = Vec::with_capacity(max_output.min(compressed.len().saturating_mul(4)));

    // When expected_size is None (unknown decompressed size), lzma-rs will
    // decompress until it hits the EOS marker. If there are trailing bytes
    // after the EOS marker (CRC, padding, etc.), lzma-rs rejects them with
    // "Found end-of-stream marker but more bytes are available."
    //
    // We work around this by catching the specific error and returning the
    // output that was successfully decompressed before the error. The lzma-rs
    // decoder writes valid data to `output` before failing on the trailing bytes.
    let mut reader = std::io::BufReader::new(std::io::Cursor::new(&lzma_header));
    match lzma_rs::lzma_decompress(&mut reader, &mut output) {
        Ok(()) => {}
        Err(e) => {
            let msg = e.to_string();
            // If we got data and the error is about trailing bytes, that's OK —
            // the LZMA stream was fully decoded, just with leftover input.
            if !output.is_empty() && msg.contains("more bytes are available") {
                // Successfully decoded up to the EOS marker.
            } else {
                return Err(Error::DecompressionFailed {
                    method: "lzma",
                    detail: msg,
                });
            }
        }
    }

    if output.len() > max_output {
        output.truncate(max_output);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short_input() {
        let result = decompress_lzma(&[0x5D, 0x00, 0x00], 1024, None);
        assert!(result.is_err());
    }

    /// Real per-file LZMA payload from an NSIS 2 LZMA non-solid installer:
    /// a 5-byte props header (`0x5D`, 8 MiB dict) followed by a stream that
    /// terminates with an end-of-stream marker, decompressing to a 1430-byte
    /// icon. NSIS non-solid streams do not store the uncompressed size, so
    /// the decoder must rely on the EOS marker.
    const NSIS_EOS_STREAM: &[u8] = include_bytes!("../../tests/fixtures/lzma_eos_marker_file.bin");

    #[test]
    fn eos_marker_stream_decompresses_with_unknown_size() {
        // `None` (unknown size) lets the decoder honor the EOS marker.
        let out = decompress_lzma(NSIS_EOS_STREAM, 64 * 1024 * 1024, None)
            .expect("EOS-terminated stream should decode with unknown size");
        assert_eq!(out.len(), 1430, "decompressed size should match the icon");
        assert_eq!(&out[0..4], &[0x00, 0x00, 0x01, 0x00], "should be a valid .ico header");
    }

    #[test]
    fn fixed_size_larger_than_actual_rejects_eos_marker() {
        // Regression: passing a fixed expected size larger than the true output
        // (as the file decompressor used to do with `Some(max_output)`) makes
        // lzma-rs reject the early EOS marker. This is exactly the failure that
        // dropped real NSIS LZMA files from extraction.
        let result = decompress_lzma(NSIS_EOS_STREAM, 64 * 1024 * 1024, Some(64 * 1024 * 1024));
        assert!(
            result.is_err(),
            "an over-large fixed size must not silently succeed on an EOS-terminated stream"
        );
    }
}
