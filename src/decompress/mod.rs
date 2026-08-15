//! Decompression support for NSIS installer data.
//!
//! NSIS uses three compression methods: zlib/deflate, bzip2, and LZMA.
//! The header block and individual data files are compressed with a common
//! framing format: a 4-byte length prefix where bit 31 indicates whether
//! the data is compressed.
//!
//! # Compression Modes
//!
//! - **Non-solid**: Header is compressed independently; each data file is a
//!   separate compressed stream with its own length prefix.
//! - **Solid** (`/SOLID`): The entire overlay (header + all data files) is
//!   a single compressed stream.

pub mod bzip2;
pub mod deflate;
pub mod lzma;

use core::fmt;

use crate::error::Error;

/// Identifies the compression algorithm used by an NSIS installer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    /// Raw deflate (no zlib header).
    Deflate,
    /// NSIS custom bzip2 (no standard `"BZ"` file header).
    Bzip2,
    /// LZMA compression.
    Lzma,
    /// Data is stored uncompressed.
    None,
}

/// Whether the installer uses solid or non-solid compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMode {
    /// All data in a single compressed stream.
    Solid,
    /// Each block compressed independently.
    NonSolid,
}

impl fmt::Display for CompressionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CompressionMethod::Deflate => "deflate",
            CompressionMethod::Bzip2 => "bzip2",
            CompressionMethod::Lzma => "lzma",
            CompressionMethod::None => "none",
        };
        f.write_str(s)
    }
}

impl fmt::Display for CompressionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CompressionMode::Solid => "solid",
            CompressionMode::NonSolid => "non-solid",
        };
        f.write_str(s)
    }
}

/// Reads a 4-byte NSIS length prefix.
///
/// Returns `(is_compressed, size)` where:
/// - `is_compressed`: `true` if bit 31 is set (data follows as compressed bytes)
/// - `size`: the lower 31 bits, giving the byte count
///
/// # Errors
///
/// Returns [`Error::TooShort`] if `data.len() < 4`.
pub fn read_length_prefix(data: &[u8]) -> Result<(bool, u32), Error> {
    if data.len() < 4 {
        return Err(Error::TooShort {
            expected: 4,
            actual: data.len(),
            context: "length prefix",
        });
    }
    let raw = crate::util::read_u32_le(data, 0);
    let is_compressed = raw & 0x8000_0000 != 0;
    let size = raw & 0x7FFF_FFFF;
    Ok((is_compressed, size))
}

/// Detects the compression method from the initial bytes of compressed data.
///
/// Detection heuristics:
/// - LZMA: first byte is typically `0x5D` followed by 4-byte dictionary size
/// - bzip2: first byte is `0x31` (NSIS custom bzip2 block header)
/// - Deflate: fallback — try raw deflate decompression
pub fn detect_compression(data: &[u8]) -> CompressionMethod {
    if data.is_empty() {
        return CompressionMethod::None;
    }

    // LZMA: properties byte is typically 0x5D (lc=3, lp=0, pb=2).
    if data.first().copied() == Some(0x5D) && data.len() >= 5 {
        return CompressionMethod::Lzma;
    }

    // NSIS bzip2 starts with a block magic that differs from standard bzip2.
    // The first byte of an NSIS bzip2 stream is '1' (0x31) for the block size digit.
    if data.first().copied() == Some(0x31) && data.len() >= 4 {
        return CompressionMethod::Bzip2;
    }

    // Default to deflate.
    CompressionMethod::Deflate
}

/// How a decompressor should bound its output.
///
/// Replaces an ambiguous `(max_output, expected_size)` pair with the three
/// concrete intents an NSIS decode actually needs. Each carries the byte size
/// it is parameterized by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeLimit {
    /// The decompressed size is known exactly: produce up to `n` bytes then
    /// stop, ignoring any trailing input.
    ///
    /// Used for header blocks, whose size is recorded in the NSIS structures.
    /// For a solid header the same stream continues with file data afterward,
    /// so the decode must stop precisely rather than consume the remainder.
    Exact(usize),
    /// The size is unknown: decode to the natural end of stream; if the output
    /// would exceed `n`, fail with [`Error::OutputTooLarge`].
    ///
    /// Used for extracted files and uninstaller overlays — an over-budget
    /// artifact is rejected, never stored truncated.
    Capped(usize),
    /// The size is unknown: decode to the natural end of stream, but stop at
    /// `n` bytes without error.
    ///
    /// Used for the solid working buffer, which is indexed by offset to slice
    /// out individual files. Over-budget (or over-reading) streams are bounded
    /// rather than failing the whole installer.
    ///
    /// A truncated buffer is indistinguishable from a complete one by length
    /// alone, so callers that need to tell them apart must read
    /// [`Decoded::truncated`] on the decode result. The parser
    /// does this for the solid stream and reports the outcome through
    /// [`NsisInstaller::solid_status`](crate::installer::NsisInstaller::solid_status);
    /// files that fall past the truncation point then fail with
    /// [`Error::OutputTooLarge`] rather than a misleading bounds error.
    Truncate(usize),
}

/// The result of a decode that may stop early at its budget.
///
/// Returned by [`decompress_block`] and the per-codec entry points. Callers
/// that only want the bytes can take [`data`](Self::data) and ignore the flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The decompressed bytes.
    pub data: Vec<u8>,
    /// `true` if decoding stopped because the limit was reached while input
    /// remained, rather than because the stream ended.
    ///
    /// Only [`DecodeLimit::Exact`] and [`DecodeLimit::Truncate`] can set this:
    /// [`DecodeLimit::Capped`] rejects an over-budget stream with
    /// [`Error::OutputTooLarge`] instead of returning a partial buffer.
    pub truncated: bool,
}

impl Decoded {
    /// Wraps bytes that were decoded in full.
    #[inline]
    pub(crate) fn complete(data: Vec<u8>) -> Self {
        Self {
            data,
            truncated: false,
        }
    }
}

impl DecodeLimit {
    /// Returns the byte size the limit is parameterized by.
    #[inline]
    pub fn size(self) -> usize {
        match self {
            DecodeLimit::Exact(n) | DecodeLimit::Capped(n) | DecodeLimit::Truncate(n) => n,
        }
    }
}

/// Decompresses a single NSIS data block.
///
/// # Arguments
///
/// - `data`: the compressed bytes (after the 4-byte length prefix)
/// - `method`: the compression algorithm to use
/// - `limit`: how the output is bounded — see [`DecodeLimit`]
///
/// # Returns
///
/// A [`Decoded`] whose [`truncated`](Decoded::truncated) flag distinguishes a
/// buffer that ended naturally from one that stopped at a
/// [`DecodeLimit::Truncate`] budget. Callers that index into the result (the
/// solid file-data stream) need this to tell truncation apart from corruption.
///
/// # Errors
///
/// Returns [`Error::DecompressionFailed`] if decompression fails, or
/// [`Error::OutputTooLarge`] if a [`DecodeLimit::Capped`] stream exceeds its
/// budget.
pub fn decompress_block(
    data: &[u8],
    method: CompressionMethod,
    limit: DecodeLimit,
) -> Result<Decoded, Error> {
    match method {
        CompressionMethod::Deflate => deflate::decompress_deflate(data, limit),
        CompressionMethod::Bzip2 => bzip2::decompress_bzip2(data, limit),
        CompressionMethod::Lzma => lzma::decompress_lzma(data, limit),
        CompressionMethod::None => Ok(Decoded::complete(data.to_vec())),
    }
}

/// Decompresses the NSIS header block following the FirstHeader.
///
/// This function:
/// 1. Reads the 4-byte length prefix
/// 2. Detects the compression method
/// 3. Decompresses the header data
/// 4. Determines whether this is solid or non-solid compression
///
/// Returns `(decompressed_data, method, mode, header_bytes_consumed)` where
/// `header_bytes_consumed` is the number of bytes from `data` occupied by
/// the compressed (or uncompressed) header. For non-solid mode, the data
/// block starts immediately after these bytes.
///
/// # Arguments
///
/// - `data`: bytes starting immediately after the FirstHeader
/// - `expected_size`: the decompressed header size from `FirstHeader::length_of_header()`
///
/// # Errors
///
/// Returns an error if decompression fails with all supported methods.
pub fn decompress_header(
    data: &[u8],
    expected_size: usize,
) -> Result<(Vec<u8>, CompressionMethod, CompressionMode, usize), Error> {
    // First, try non-solid mode: the header starts with a length prefix.
    // If the length prefix produces values that don't fit, we skip to solid mode.
    let (is_compressed, size) = read_length_prefix(data)?;

    let size_usize = size as usize;
    let payload_end = 4_usize.checked_add(size_usize);
    if !is_compressed && payload_end.is_some_and(|end| end <= data.len()) {
        // Data is uncompressed — just take the raw bytes.
        let bytes = data.get(4..).and_then(|s| s.get(..size_usize));
        if let Some(bytes) = bytes {
            return Ok((
                bytes.to_vec(),
                CompressionMethod::None,
                CompressionMode::NonSolid,
                payload_end.unwrap_or(0),
            ));
        }
    }

    let compressed_size = size_usize;
    let non_solid_consumed = 4_usize.saturating_add(compressed_size);
    let non_solid_viable = is_compressed && data.len() >= non_solid_consumed;
    let compressed_data: &[u8] = if non_solid_viable {
        data.get(4..non_solid_consumed).unwrap_or(&[])
    } else {
        &[]
    };

    // Try to detect and decompress with non-solid framing.
    //
    // The header's decompressed size is known (`expected_size`), so we request
    // an [`DecodeLimit::Exact`] decode: take exactly that many bytes and ignore
    // any trailing input. Block-based codecs (deflate/bzip2) may decode a whole
    // block that overshoots the header, and LZMA frames may carry trailing
    // bytes after the EOS marker — an exact bound sidesteps both.
    let method = detect_compression(compressed_data);
    if let Ok(decoded) =
        decompress_block(compressed_data, method, DecodeLimit::Exact(expected_size))
        && !decoded.data.is_empty()
    {
        return Ok((
            decoded.data,
            method,
            CompressionMode::NonSolid,
            non_solid_consumed,
        ));
    }

    // If the detected method failed, try the other methods.
    let methods = [
        CompressionMethod::Lzma,
        CompressionMethod::Deflate,
        CompressionMethod::Bzip2,
    ];
    for &m in &methods {
        if m == method {
            continue;
        }
        if let Ok(decoded) = decompress_block(compressed_data, m, DecodeLimit::Exact(expected_size))
            && !decoded.data.is_empty()
        {
            return Ok((
                decoded.data,
                m,
                CompressionMode::NonSolid,
                non_solid_consumed,
            ));
        }
    }

    // Non-solid decompression failed entirely.
    // Try solid mode: the entire post-FirstHeader data is one compressed stream.
    // For solid LZMA, trailing data (data block, CRC) follows the header's EOS
    // marker, so we must provide the exact expected size to avoid lzma-rs
    // rejecting the stream for having trailing bytes.
    //
    // In solid mode the decompressed stream is framed: each sub-block starts
    // with a 4-byte LE length prefix. The NSIS loader (`_dodecomp` in
    // `fileform.c`) reads and consumes this prefix before returning the header
    // data. We must include those 4 bytes in the decompression output and
    // strip them afterwards.
    let solid_expected = expected_size.saturating_add(4); // account for in-stream length prefix
    let solid_method = detect_compression(data);
    if let Ok(decoded) = decompress_block(data, solid_method, DecodeLimit::Exact(solid_expected)) {
        let stripped = strip_solid_prefix(decoded.data)?;
        // Solid: the entire stream is one blob, no separate data block offset.
        return Ok((stripped, solid_method, CompressionMode::Solid, 0));
    }

    for &m in &methods {
        if m == solid_method {
            continue;
        }
        if let Ok(decoded) = decompress_block(data, m, DecodeLimit::Exact(solid_expected)) {
            let stripped = strip_solid_prefix(decoded.data)?;
            return Ok((stripped, m, CompressionMode::Solid, 0));
        }
    }

    Err(Error::UnsupportedCompression)
}

/// Strips the 4-byte in-stream length prefix from solid-mode decompressed data.
///
/// In solid mode, the NSIS decompressed stream starts with a 4-byte LE integer
/// containing the size of the following header data. The NSIS runtime loader
/// (`_dodecomp` in `fileform.c`) reads and discards this prefix. We do the same.
///
/// Validates that the prefix value matches the remaining data length.
fn strip_solid_prefix(data: Vec<u8>) -> Result<Vec<u8>, Error> {
    if data.len() < 4 {
        return Err(Error::TooShort {
            expected: 4,
            actual: data.len(),
            context: "solid stream length prefix",
        });
    }
    let prefix = crate::util::read_u32_le(&data, 0) as usize;
    if prefix == data.len().saturating_sub(4) {
        // Prefix matches exactly — strip it.
        Ok(data.get(4..).unwrap_or(&[]).to_vec())
    } else {
        // Prefix doesn't match. This can happen if the data isn't actually
        // solid-framed (e.g., some NSIS versions omit the prefix). Return
        // the data as-is and let the caller validate.
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_length_prefix_compressed() {
        // bit 31 set, lower 31 bits = 1000
        let val = 0x8000_0000u32 | 1000;
        let data = val.to_le_bytes();
        let (is_compressed, size) = read_length_prefix(&data).unwrap();
        assert!(is_compressed);
        assert_eq!(size, 1000);
    }

    #[test]
    fn read_length_prefix_uncompressed() {
        let val = 2048u32;
        let data = val.to_le_bytes();
        let (is_compressed, size) = read_length_prefix(&data).unwrap();
        assert!(!is_compressed);
        assert_eq!(size, 2048);
    }

    #[test]
    fn read_length_prefix_too_short() {
        let data = [0u8; 3];
        assert!(read_length_prefix(&data).is_err());
    }

    #[test]
    fn detect_compression_lzma() {
        let data = [0x5D, 0x00, 0x00, 0x01, 0x00, 0xFF];
        assert_eq!(detect_compression(&data), CompressionMethod::Lzma);
    }

    #[test]
    fn detect_compression_bzip2() {
        let data = [0x31, 0x41, 0x59, 0x26];
        assert_eq!(detect_compression(&data), CompressionMethod::Bzip2);
    }

    #[test]
    fn detect_compression_deflate_fallback() {
        let data = [0x78, 0x9C, 0x01, 0x02];
        assert_eq!(detect_compression(&data), CompressionMethod::Deflate);
    }

    #[test]
    fn detect_compression_empty() {
        assert_eq!(detect_compression(&[]), CompressionMethod::None);
    }

    #[test]
    fn decompress_header_uncompressed() {
        let payload = b"hello world test data";
        let size = payload.len() as u32;
        let mut data = Vec::new();
        data.extend_from_slice(&size.to_le_bytes());
        data.extend_from_slice(payload);
        let (decompressed, method, mode, consumed) =
            decompress_header(&data, payload.len()).unwrap();
        assert_eq!(&decompressed, payload);
        assert_eq!(method, CompressionMethod::None);
        assert_eq!(mode, CompressionMode::NonSolid);
        assert_eq!(consumed, 4 + payload.len());
    }
}
