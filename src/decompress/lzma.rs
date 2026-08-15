//! LZMA decompression for NSIS data blocks.
//!
//! NSIS LZMA streams begin with a properties byte (typically `0x5D` for
//! lc=3, lp=0, pb=2) followed by a 4-byte little-endian dictionary size.

use std::io::{self, Write};

use crate::{decompress::DecodeLimit, error::Error};

/// A [`Write`] sink that appends to an in-memory buffer but refuses to grow
/// past a fixed byte budget.
///
/// This bounds the LZMA decoder *during* decompression (a true
/// memory-exhaustion guard) rather than decoding fully and truncating
/// afterward. When the budget is reached the partial bytes are kept,
/// `overflowed` is set, and the write fails to stop the decoder — the caller
/// then either rejects ([`DecodeLimit::Capped`]) or keeps the truncated buffer
/// ([`DecodeLimit::Truncate`]).
struct LimitedWriter {
    buf: Vec<u8>,
    limit: usize,
    overflowed: bool,
}

impl Write for LimitedWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = self.limit.saturating_sub(self.buf.len());
        if data.len() > room {
            // Fill up to the limit (so `Truncate` keeps a full buffer), then
            // fail to halt the decoder.
            if let Some(head) = data.get(..room) {
                self.buf.extend_from_slice(head);
            }
            self.overflowed = true;
            return Err(io::Error::other("decompressed output exceeds limit"));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Decompresses an NSIS LZMA stream.
///
/// NSIS LZMA streams use the raw LZMA format: a properties byte followed by a
/// 4-byte dictionary size, then the compressed data. The uncompressed size is
/// not stored in the NSIS LZMA header.
///
/// # Arguments
///
/// - `compressed`: the raw LZMA stream (properties byte + 4-byte dict size + data)
/// - `limit`: how the output is bounded — see [`DecodeLimit`]. The
///   unknown-size variants ([`DecodeLimit::Capped`] / [`DecodeLimit::Truncate`])
///   set the header size to "unknown" and rely on the EOS marker; only
///   [`DecodeLimit::Exact`] writes a fixed size into the LZMA header.
///
/// # Errors
///
/// Returns [`Error::DecompressionFailed`] if the LZMA stream is invalid, or
/// [`Error::OutputTooLarge`] if a [`DecodeLimit::Capped`] stream exceeds its
/// budget.
pub fn decompress_lzma(compressed: &[u8], limit: DecodeLimit) -> Result<Vec<u8>, Error> {
    if compressed.len() < 5 {
        return Err(Error::DecompressionFailed {
            method: "lzma",
            detail: "LZMA stream too short (need at least 5 bytes for header)".into(),
        });
    }

    // Build a standard LZMA header for lzma-rs:
    // Bytes 0:   properties byte (from NSIS stream)
    // Bytes 1-4: dictionary size (from NSIS stream)
    // Bytes 5-12: uncompressed size. Only `Exact` knows it; the unknown-size
    //             variants use the 0xFFFF... sentinel and rely on the EOS marker.
    let uncompressed_size_bytes: [u8; 8] = match limit {
        DecodeLimit::Exact(size) => (size as u64).to_le_bytes(),
        DecodeLimit::Capped(_) | DecodeLimit::Truncate(_) => [0xFF; 8],
    };

    let mut lzma_header = Vec::with_capacity(compressed.len().saturating_add(8));
    let (props, body) = compressed.split_at(5);
    lzma_header.extend_from_slice(props); // props + dict_size
    lzma_header.extend_from_slice(&uncompressed_size_bytes);
    lzma_header.extend_from_slice(body);

    let max_output = limit.size();
    let capacity = max_output.min(compressed.len().saturating_mul(4));
    let mut writer = LimitedWriter {
        buf: Vec::with_capacity(capacity),
        limit: max_output,
        overflowed: false,
    };

    // The decoder writes into a budget-bounded sink. For the unknown-size
    // variants lzma-rust2 decompresses until the EOS marker (the LzmaReader
    // consumes the LZMA alone header we built above); trailing bytes after
    // the marker (CRC, padding) are left unread, so unlike lzma-rs there is
    // no "more bytes are available" error to tolerate.
    let mut reader = lzma_rust2::LzmaReader::new_mem_limit(
        std::io::Cursor::new(&lzma_header),
        u32::MAX,
        None,
    )
    .map_err(|e| Error::DecompressionFailed {
        method: "lzma",
        detail: e.to_string(),
    })?;
    match std::io::copy(&mut reader, &mut writer) {
        Ok(_) => {}
        Err(e) => {
            if writer.overflowed {
                // Budget reached mid-decode: reject or keep the truncated buffer.
                match limit {
                    DecodeLimit::Capped(n) => return Err(Error::OutputTooLarge { limit: n }),
                    DecodeLimit::Truncate(_) | DecodeLimit::Exact(_) => {}
                }
            } else {
                let msg = e.to_string();
                // If we got data and the error is about trailing bytes, that's
                // OK — the LZMA stream was fully decoded, just with leftover input.
                if !writer.buf.is_empty() && msg.contains("more bytes are available") {
                    // Successfully decoded up to the EOS marker.
                } else {
                    return Err(Error::DecompressionFailed {
                        method: "lzma",
                        detail: msg,
                    });
                }
            }
        }
    }

    Ok(writer.buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_short_input() {
        let result = decompress_lzma(&[0x5D, 0x00, 0x00], DecodeLimit::Capped(1024));
        assert!(result.is_err());
    }

    /// Real per-file LZMA payload from an NSIS 2 LZMA non-solid installer:
    /// a 5-byte props header (`0x5D`, 8 MiB dict) followed by a stream that
    /// terminates with an end-of-stream marker, decompressing to a 1430-byte
    /// icon. NSIS non-solid streams do not store the uncompressed size, so
    /// the decoder must rely on the EOS marker.
    const NSIS_EOS_STREAM: &[u8] = include_bytes!("../../tests/fixtures/lzma_eos_marker_file.bin");

    #[test]
    fn eos_marker_stream_decompresses_when_capped() {
        // `Capped` (unknown size) lets the decoder honor the EOS marker.
        let out = decompress_lzma(NSIS_EOS_STREAM, DecodeLimit::Capped(64 * 1024 * 1024))
            .expect("EOS-terminated stream should decode with unknown size");
        assert_eq!(out.len(), 1430, "decompressed size should match the icon");
        assert_eq!(
            out.get(..4),
            Some(&[0x00, 0x00, 0x01, 0x00][..]),
            "should be a valid .ico header"
        );
    }

    #[test]
    fn exact_size_larger_than_actual_rejects_eos_marker() {
        // Regression: an `Exact` size larger than the true output (as the file
        // decompressor used to effectively pass) makes lzma-rs reject the early
        // EOS marker. This is exactly the failure that dropped real NSIS LZMA
        // files from extraction — unknown-size streams must use `Capped`.
        let result = decompress_lzma(NSIS_EOS_STREAM, DecodeLimit::Exact(64 * 1024 * 1024));
        assert!(
            result.is_err(),
            "an over-large exact size must not silently succeed on an EOS-terminated stream"
        );
    }

    #[test]
    fn capped_rejects_when_budget_below_actual() {
        // The real output is 1430 bytes; a 512-byte cap must be rejected.
        let result = decompress_lzma(NSIS_EOS_STREAM, DecodeLimit::Capped(512));
        assert!(matches!(result, Err(Error::OutputTooLarge { limit: 512 })));
    }

    #[test]
    fn truncate_caps_without_error() {
        // Same under-budget stream, but `Truncate` keeps the first 512 bytes.
        let out = decompress_lzma(NSIS_EOS_STREAM, DecodeLimit::Truncate(512)).unwrap();
        assert_eq!(out.len(), 512);
    }
}
