//! Raw deflate decompression for NSIS data blocks.
//!
//! NSIS uses raw deflate (no zlib header) for its default compression.

use std::io::Read;

use crate::{decompress::DecodeLimit, error::Error};

/// Chunk size for streaming the unknown-size decode paths.
const STREAM_CHUNK: usize = 64 * 1024;

/// Decompresses raw deflate data.
///
/// NSIS uses raw deflate (RFC 1951) without zlib or gzip framing.
///
/// # Arguments
///
/// - `compressed`: the raw deflate stream (no zlib/gzip framing)
/// - `limit`: how the output is bounded — see [`DecodeLimit`]
///
/// # Errors
///
/// Returns [`Error::DecompressionFailed`] if the deflate stream is invalid, or
/// [`Error::OutputTooLarge`] if a [`DecodeLimit::Capped`] stream exceeds its
/// budget.
pub fn decompress_deflate(compressed: &[u8], limit: DecodeLimit) -> Result<Vec<u8>, Error> {
    match limit {
        DecodeLimit::Exact(n) => decompress_bounded(compressed, n),
        DecodeLimit::Capped(n) => decompress_streaming(compressed, n, false),
        DecodeLimit::Truncate(n) => decompress_streaming(compressed, n, true),
    }
}

/// Bounded decode: fill an `n`-byte buffer and stop, ignoring trailing input.
fn decompress_bounded(compressed: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    let mut decompressor = flate2::Decompress::new(false); // raw deflate, no zlib header
    let mut output = vec![0u8; limit];

    // `BufError` here means the output buffer filled before the input was
    // consumed — expected when more (unwanted) data follows the bounded
    // region, so we keep what we decoded.
    let status = decompressor
        .decompress(compressed, &mut output, flate2::FlushDecompress::Finish)
        .map_err(|e| Error::DecompressionFailed {
            method: "deflate",
            detail: e.to_string(),
        })?;

    let bytes_written = decompressor.total_out() as usize;
    match status {
        flate2::Status::Ok | flate2::Status::StreamEnd | flate2::Status::BufError => {
            output.truncate(bytes_written);
            Ok(output)
        }
    }
}

/// Streaming decode to end-of-stream. On exceeding `max_output`, either stop
/// (`truncate = true`) or fail with [`Error::OutputTooLarge`].
fn decompress_streaming(
    compressed: &[u8],
    max_output: usize,
    truncate: bool,
) -> Result<Vec<u8>, Error> {
    let mut decoder = flate2::read::DeflateDecoder::new(compressed);
    let mut output = Vec::new();
    let mut chunk = [0u8; STREAM_CHUNK];

    loop {
        let read = decoder
            .read(&mut chunk)
            .map_err(|e| Error::DecompressionFailed {
                method: "deflate",
                detail: e.to_string(),
            })?;
        if read == 0 {
            break;
        }
        if let Some(bytes) = chunk.get(..read) {
            output.extend_from_slice(bytes);
        }
        if output.len() > max_output {
            if truncate {
                output.truncate(max_output);
                break;
            }
            return Err(Error::OutputTooLarge { limit: max_output });
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compress;

    #[test]
    fn roundtrip_deflate() {
        let original = b"Hello, NSIS installer world! This is test data for deflate.";
        let mut compressed = vec![0u8; original.len() + 64];
        let mut compressor = Compress::new(flate2::Compression::default(), false);
        let status = compressor
            .compress(original, &mut compressed, flate2::FlushCompress::Finish)
            .unwrap();
        assert_eq!(status, flate2::Status::StreamEnd);
        let compressed_len = compressor.total_out() as usize;
        compressed.truncate(compressed_len);

        // Capped decode (unknown size) round-trips.
        let capped = decompress_deflate(&compressed, DecodeLimit::Capped(original.len())).unwrap();
        assert_eq!(&capped, original);

        // Exact decode (known size) round-trips.
        let exact = decompress_deflate(&compressed, DecodeLimit::Exact(original.len())).unwrap();
        assert_eq!(&exact, original);
    }

    #[test]
    fn invalid_deflate_data() {
        let garbage = [0xFF, 0xFE, 0xFD, 0xFC];
        let result = decompress_deflate(&garbage, DecodeLimit::Capped(1024));
        assert!(result.is_err());
    }

    /// Builds a deflate stream that decompresses to `len` zero bytes.
    fn deflate_zeros(len: usize) -> Vec<u8> {
        let original = vec![0u8; len];
        let mut compressed = vec![0u8; len + 1024];
        let mut compressor = Compress::new(flate2::Compression::default(), false);
        let status = compressor
            .compress(&original, &mut compressed, flate2::FlushCompress::Finish)
            .unwrap();
        assert_eq!(status, flate2::Status::StreamEnd);
        let compressed_len = compressor.total_out() as usize;
        compressed.truncate(compressed_len);
        compressed
    }

    #[test]
    fn capped_decode_rejects_oversized_output() {
        let compressed = deflate_zeros(256 * 1024); // expands past a tiny cap
        let result = decompress_deflate(&compressed, DecodeLimit::Capped(4096));
        assert!(matches!(result, Err(Error::OutputTooLarge { limit: 4096 })));
    }

    #[test]
    fn truncate_decode_caps_without_error() {
        let compressed = deflate_zeros(256 * 1024);
        let out = decompress_deflate(&compressed, DecodeLimit::Truncate(4096)).unwrap();
        assert_eq!(out.len(), 4096);
    }
}
