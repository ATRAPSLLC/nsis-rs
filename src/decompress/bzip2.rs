//! NSIS-modified bzip2 decompressor.
//!
//! This module implements the NSIS-specific bzip2 decompression format, which
//! differs from standard bzip2 in several important ways:
//!
//! - **No file header**: standard bzip2 starts with `"BZh"` followed by a block
//!   size digit; NSIS bzip2 streams start directly with block data.
//! - **Simplified block header**: a single byte `0x31` signals a data block,
//!   `0x17` signals end-of-stream. Standard bzip2 uses 6-byte block headers
//!   with `0x314159265359` (pi) and `0x177245385090` (sqrt(pi)).
//! - **No per-block CRC32**: standard bzip2 includes a 32-bit CRC after each
//!   block header; NSIS omits it entirely.
//! - **No randomised flag**: the 1-bit randomised flag present in standard bzip2
//!   block headers is absent.
//! - **Fixed block size**: hardcoded to 900,000 bytes (equivalent to standard
//!   bzip2 level 9).
//!
//! The reference C implementation lives in the NSIS source tree at
//! `Source/bzip2/decompress.c` and `Source/bzip2/huffman.c`. That code uses a
//! resumable state machine (with `switch`/`goto`) so the decompressor can yield
//! when the input buffer is exhausted. Since we always have the complete input
//! buffer available, this Rust port restructures the logic as a straightforward
//! blocking decoder that reads from a `BitReader`.
//!
//! # References
//!
//! - NSIS source: `Source/bzip2/bzlib.h`, `decompress.c`, `huffman.c`
//! - Original bzip2: Julian Seward, <http://www.bzip.org/>
//!
//! # Lint allowlist
//!
//! This module is a direct port of a vendored decompressor algorithm. All
//! arithmetic and indexing operations in the inner Huffman / BWT loops are
//! against fixed-size internal state arrays (`BZ_MAX_ALPHA_SIZE`,
//! `BZ_N_GROUPS`, `MTFA_SIZE`, `BLOCK_SIZE`) and are guarded by explicit
//! input-range checks at the block boundaries (origPtr, nGroups,
//! nSelectors, code lengths). Wrapping every internal `+`/`<<`/`arr[i]`
//! with `saturating_*` / `.get()` would obscure the algorithm without
//! adding safety: any residual out-of-bounds index would still produce
//! a controlled Rust panic rather than UB.
//!
//! To make sure such a panic never escapes into a downstream
//! malware-analysis pipeline, the public entry point [`decompress_bzip2`]
//! wraps the decoder in [`std::panic::catch_unwind`] and converts any
//! captured panic into a [`DecompressionFailed`](Error::DecompressionFailed)
//! error.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use crate::{decompress::DecodeLimit, error::Error};

// ---------------------------------------------------------------------------
// Constants (from bzlib.h)
// ---------------------------------------------------------------------------

/// Maximum alphabet size: 256 byte values + RUNA + RUNB.
const BZ_MAX_ALPHA_SIZE: usize = 258;

/// Maximum Huffman code length in bits.
const BZ_MAX_CODE_LEN: usize = 23;

/// Maximum number of Huffman groups per block.
const BZ_N_GROUPS: usize = 6;

/// Number of symbols per Huffman group selector.
const BZ_G_SIZE: usize = 50;

/// Maximum number of selectors: `2 + (900000 / BZ_G_SIZE)`.
const BZ_MAX_SELECTORS: usize = 18002;

/// MTF array size for the fast MTF decoder.
const MTFA_SIZE: usize = 4096;

/// MTF list size (number of sub-lists of 16 entries each).
const MTFL_SIZE: usize = 16;

/// Block size in bytes: NSIS hardcodes level 9 = 900,000.
const BLOCK_SIZE: usize = 900_000;

/// Run-length symbol A.
const BZ_RUNA: i32 = 0;

/// Run-length symbol B.
const BZ_RUNB: i32 = 1;

// ---------------------------------------------------------------------------
// BitReader --- reads bits from a byte slice, MSB first
// ---------------------------------------------------------------------------

/// Reads bits from a byte slice, most-significant bit first, matching the
/// bzip2 bitstream convention.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    live: i32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            live: 0,
        }
    }

    /// Reads `n` bits (1..=24) and returns them as the low `n` bits of a `u32`.
    fn get_bits(&mut self, n: i32) -> Result<i32, Error> {
        loop {
            if self.live >= n {
                let v = (self.buf >> (self.live - n)) & ((1 << n) - 1);
                self.live -= n;
                return Ok(v as i32);
            }
            if self.pos >= self.data.len() {
                return Err(fail("unexpected end of input"));
            }
            self.buf = (self.buf << 8) | (self.data[self.pos] as u32);
            self.live += 8;
            self.pos += 1;
        }
    }

    /// Reads a single bit.
    #[inline]
    fn get_bit(&mut self) -> Result<i32, Error> {
        self.get_bits(1)
    }

    /// Reads an 8-bit unsigned value.
    #[inline]
    fn get_u8(&mut self) -> Result<i32, Error> {
        self.get_bits(8)
    }
}

// ---------------------------------------------------------------------------
// Huffman decode tables (port of BZ2_hbCreateDecodeTables)
// ---------------------------------------------------------------------------

/// Builds Huffman decoding tables from code lengths.
///
/// This is a direct port of `BZ2_hbCreateDecodeTables()` from `huffman.c`.
///
/// # Arguments
///
/// - `limit`: output array --- `limit[i]` is the largest code value of length `i`
/// - `base`: output array --- used to map codes to symbol indices
/// - `perm`: output array --- permutation mapping decoded index to symbol
/// - `length`: input array --- code length for each symbol
/// - `min_len`, `max_len`: minimum and maximum code lengths
/// - `alpha_size`: number of symbols in the alphabet
fn create_decode_tables(
    limit: &mut [i32],
    base: &mut [i32],
    perm: &mut [i32],
    length: &[u8],
    min_len: i32,
    max_len: i32,
    alpha_size: usize,
) {
    let mut pp = 0usize;
    for i in min_len..=max_len {
        for (j, &len_j) in length.iter().enumerate().take(alpha_size) {
            if len_j as i32 == i {
                perm[pp] = j as i32;
                pp += 1;
            }
        }
    }

    for item in base.iter_mut().take(BZ_MAX_CODE_LEN) {
        *item = 0;
    }
    for &len_j in length.iter().take(alpha_size) {
        let idx = len_j as usize + 1;
        if idx < BZ_MAX_CODE_LEN {
            base[idx] += 1;
        }
    }

    for i in 1..BZ_MAX_CODE_LEN {
        base[i] += base[i - 1];
    }

    for item in limit.iter_mut().take(BZ_MAX_CODE_LEN) {
        *item = 0;
    }
    let mut vec: i32 = 0;

    for i in min_len..=max_len {
        let iu = i as usize;
        vec += base[iu + 1] - base[iu];
        limit[iu] = vec - 1;
        vec <<= 1;
    }
    for i in (min_len + 1)..=max_len {
        let iu = i as usize;
        base[iu] = ((limit[iu - 1] + 1) << 1) - base[iu];
    }
}

// ---------------------------------------------------------------------------
// BWT inverse transform output (port of BZ_GET_FAST macro)
// ---------------------------------------------------------------------------

/// Performs one step of the BWT inverse transform (fast variant).
///
/// Equivalent to the `BZ_GET_FAST` macro:
/// ```c
/// s->tPos = s->tt[s->tPos];
/// cccc = (UChar)(s->tPos & 0xff);
/// s->tPos >>= 8;
/// ```
#[inline]
fn bz_get_fast(tt: &[u32], t_pos: &mut u32) -> u8 {
    *t_pos = tt[*t_pos as usize];
    let ch = (*t_pos & 0xff) as u8;
    *t_pos >>= 8;
    ch
}

// ---------------------------------------------------------------------------
// Huffman table parameters struct (avoids too-many-arguments on get_mtf_val)
// ---------------------------------------------------------------------------

/// Groups the Huffman decoding tables and selector state needed by
/// [`get_mtf_val`], avoiding a long parameter list.
struct HuffmanTables {
    selector: Vec<u8>,
    min_lens: [i32; BZ_N_GROUPS],
    limit: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    perm: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    base: [[i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
    n_selectors: i32,
    group_no: i32,
    group_pos: i32,
}

// ---------------------------------------------------------------------------
// Main decompression (port of BZ2_decompress + BZ2_bzDecompress)
// ---------------------------------------------------------------------------

/// Decompresses an NSIS bzip2 stream.
///
/// NSIS bzip2 differs from standard bzip2: there is no `"BZh"` stream header,
/// no per-block CRC, and a simplified block framing. The input should be the
/// raw compressed bytes (no standard bzip2 header).
///
/// # Arguments
///
/// - `compressed`: the raw NSIS bzip2 stream (without standard header)
/// - `limit`: how the output is bounded — see [`DecodeLimit`]
///
/// # Errors
///
/// Returns [`Error::DecompressionFailed`] with `method: "bzip2"` if the stream
/// is malformed, or [`Error::OutputTooLarge`] if a [`DecodeLimit::Capped`]
/// stream exceeds its budget.
pub fn decompress_bzip2(compressed: &[u8], limit: DecodeLimit) -> Result<Vec<u8>, Error> {
    // Run the decoder under `catch_unwind` so any out-of-bounds index or
    // arithmetic overflow inside the vendored algorithm surfaces as a clean
    // `DecompressionFailed` error rather than aborting the calling worker.
    // See the module-level "Lint allowlist" doc for context.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decompress_bzip2_inner(compressed, limit)
    })) {
        Ok(result) => result,
        Err(_) => Err(fail("decoder panicked on malformed input")),
    }
}

fn decompress_bzip2_inner(compressed: &[u8], limit: DecodeLimit) -> Result<Vec<u8>, Error> {
    if compressed.is_empty() {
        return Err(fail("empty input"));
    }

    // `Exact`/`Truncate` stop cleanly at the budget and ignore the rest. `Capped`
    // decodes one byte past the budget so an over-budget (or over-reading)
    // stream is detected and rejected rather than silently truncated.
    let ceiling = match limit {
        DecodeLimit::Exact(n) | DecodeLimit::Truncate(n) => n,
        DecodeLimit::Capped(n) => n.saturating_add(1),
    };

    let mut reader = BitReader::new(compressed);
    let mut output = Vec::with_capacity(ceiling.min(BLOCK_SIZE));

    loop {
        // Read block header byte (0x31 = data block, 0x17 = end of stream).
        let header = reader.get_u8()?;
        if header == 0x17 {
            // End of stream.
            break;
        }
        if header != 0x31 {
            return Err(fail(&format!(
                "invalid block header 0x{header:02X} (expected 0x31 or 0x17)"
            )));
        }

        // Decompress one block and append its output (capped at `ceiling`).
        decompress_block(&mut reader, &mut output, ceiling)?;

        if output.len() >= ceiling {
            match limit {
                // Bounded: we have the requested bytes; stop and ignore the rest.
                DecodeLimit::Exact(n) | DecodeLimit::Truncate(n) => {
                    output.truncate(n);
                    break;
                }
                // Capped: filling to the sentinel means the stream is larger
                // than the budget allows.
                DecodeLimit::Capped(n) => return Err(Error::OutputTooLarge { limit: n }),
            }
        }
    }

    Ok(output)
}

/// Decompresses a single NSIS bzip2 data block.
///
/// Reads the block from `reader`, performs BWT inverse transform, and appends
/// the decoded bytes to `output`.
fn decompress_block(
    reader: &mut BitReader<'_>,
    output: &mut Vec<u8>,
    max_output: usize,
) -> Result<(), Error> {
    // --- Read origPtr (3 bytes, big-endian) ---
    let b0 = reader.get_u8()?;
    let b1 = reader.get_u8()?;
    let b2 = reader.get_u8()?;
    let orig_ptr = (b0 << 16) | (b1 << 8) | b2;

    if orig_ptr < 0 || orig_ptr > (10 + BLOCK_SIZE as i32) {
        return Err(fail(&format!("origPtr out of range: {orig_ptr}")));
    }

    // --- Receive the mapping table ---
    // 16 bits indicating which groups of 16 bytes are in use.
    let mut in_use16 = [false; 16];
    for item in &mut in_use16 {
        *item = reader.get_bit()? == 1;
    }

    // For each group that is in use, read 16 bits for individual bytes.
    let mut in_use = [false; 256];
    for (i, &group_used) in in_use16.iter().enumerate() {
        if group_used {
            for j in 0..16 {
                in_use[i * 16 + j] = reader.get_bit()? == 1;
            }
        }
    }

    // Build seqToUnseq mapping.
    let mut seq_to_unseq = [0u8; 256];
    let mut n_in_use: usize = 0;
    for (qi, &used) in in_use.iter().enumerate() {
        if used {
            seq_to_unseq[n_in_use] = qi as u8;
            n_in_use += 1;
        }
    }

    if n_in_use == 0 {
        return Err(fail("no symbols in use"));
    }

    let alpha_size = n_in_use + 2; // +2 for RUNA and RUNB

    // --- Read selectors ---
    let n_groups = reader.get_bits(3)?;
    if !(2..=6).contains(&n_groups) {
        return Err(fail(&format!("nGroups out of range: {n_groups}")));
    }
    let n_groups = n_groups as usize;

    let n_selectors = reader.get_bits(15)?;
    if n_selectors < 1 {
        return Err(fail("nSelectors < 1"));
    }
    let n_selectors = n_selectors as usize;
    if n_selectors > BZ_MAX_SELECTORS {
        return Err(fail(&format!("nSelectors too large: {n_selectors}")));
    }

    let mut selector_mtf = vec![0u8; n_selectors];
    for sel in selector_mtf.iter_mut() {
        let mut j = 0;
        loop {
            let bit = reader.get_bit()?;
            if bit == 0 {
                break;
            }
            j += 1;
            if j >= n_groups {
                return Err(fail("selector MTF value >= nGroups"));
            }
        }
        *sel = j as u8;
    }

    // --- Undo the MTF values for the selectors ---
    let mut selector = vec![0u8; n_selectors];
    {
        let mut pos = [0u8; BZ_N_GROUPS];
        for (v, p) in pos.iter_mut().enumerate().take(n_groups) {
            *p = v as u8;
        }
        for i in 0..n_selectors {
            let v = selector_mtf[i] as usize;
            let tmp = pos[v];
            // Shift elements right.
            for k in (1..=v).rev() {
                pos[k] = pos[k - 1];
            }
            pos[0] = tmp;
            selector[i] = tmp;
        }
    }

    // --- Read the coding tables ---
    let mut len = [[0u8; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS];
    for table in len.iter_mut().take(n_groups) {
        let mut curr = reader.get_bits(5)?;
        for slot in table.iter_mut().take(alpha_size) {
            loop {
                if !(1..=20).contains(&curr) {
                    return Err(fail(&format!("code length out of range: {curr}")));
                }
                let bit = reader.get_bit()?;
                if bit == 0 {
                    break;
                }
                let bit2 = reader.get_bit()?;
                if bit2 == 0 {
                    curr += 1;
                } else {
                    curr -= 1;
                }
            }
            *slot = curr as u8;
        }
    }

    // --- Create the Huffman decoding tables ---
    let mut huff = HuffmanTables {
        selector,
        min_lens: [0i32; BZ_N_GROUPS],
        limit: [[0i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
        perm: [[0i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
        base: [[0i32; BZ_MAX_ALPHA_SIZE]; BZ_N_GROUPS],
        n_selectors: n_selectors as i32,
        group_no: -1,
        group_pos: 0,
    };

    for (t, len_t) in len.iter().enumerate().take(n_groups) {
        let mut min_len = 32i32;
        let mut max_len = 0i32;
        for &l in len_t.iter().take(alpha_size) {
            let l = l as i32;
            if l > max_len {
                max_len = l;
            }
            if l < min_len {
                min_len = l;
            }
        }
        create_decode_tables(
            &mut huff.limit[t],
            &mut huff.base[t],
            &mut huff.perm[t],
            len_t,
            min_len,
            max_len,
            alpha_size,
        );
        huff.min_lens[t] = min_len;
    }

    // --- Decode the MTF values ---
    let eob = (n_in_use + 1) as i32;
    let nblock_max = BLOCK_SIZE;

    let mut unzftab = [0i32; 256];

    // MTF init
    let mut mtfa = [0u8; MTFA_SIZE];
    let mut mtfbase = [0usize; 256 / MTFL_SIZE];
    {
        let mut kk = MTFA_SIZE - 1;
        for ii in (0..(256 / MTFL_SIZE)).rev() {
            for jj in (0..MTFL_SIZE).rev() {
                mtfa[kk] = (ii * MTFL_SIZE + jj) as u8;
                // Protect against underflow on the very last iteration.
                kk = kk.wrapping_sub(1);
            }
            mtfbase[ii] = kk.wrapping_add(1);
        }
    }

    // Storage for the BWT block (tt array).
    let mut tt = vec![0u32; nblock_max];
    let mut nblock: usize = 0;

    // Read the first symbol.
    let mut next_sym = get_mtf_val(reader, &mut huff)?;

    loop {
        if next_sym == eob {
            break;
        }

        if next_sym == BZ_RUNA || next_sym == BZ_RUNB {
            let mut es: i32 = -1;
            let mut n_power: i32 = 1;
            while next_sym == BZ_RUNA || next_sym == BZ_RUNB {
                if next_sym == BZ_RUNA {
                    es += n_power;
                }
                n_power <<= 1;
                if next_sym == BZ_RUNB {
                    es += n_power;
                }
                next_sym = get_mtf_val(reader, &mut huff)?;
            }

            es += 1;
            let uc = seq_to_unseq[mtfa[mtfbase[0]] as usize];
            unzftab[uc as usize] += es;

            let es = es as usize;
            if nblock + es > nblock_max {
                return Err(fail("block overflow during RLE expansion"));
            }
            for _ in 0..es {
                tt[nblock] = uc as u32;
                nblock += 1;
            }
            // next_sym was already advanced by the inner loop; continue.
            continue;
        }

        // Regular symbol: MTF decode.
        if nblock >= nblock_max {
            return Err(fail("block overflow"));
        }

        let uc = mtf_decode(next_sym, &mut mtfa, &mut mtfbase)?;

        let unseq = seq_to_unseq[uc as usize];
        unzftab[unseq as usize] += 1;
        tt[nblock] = unseq as u32;
        nblock += 1;

        next_sym = get_mtf_val(reader, &mut huff)?;
    }

    // --- Validate origPtr ---
    if orig_ptr < 0 || (orig_ptr as usize) >= nblock {
        return Err(fail(&format!(
            "origPtr {orig_ptr} out of range for nblock {nblock}"
        )));
    }

    // --- Set up cftab to facilitate generation of T^(-1) ---
    let mut cftab = [0i32; 257];
    cftab[0] = 0;
    for i in 1..=256 {
        cftab[i] = unzftab[i - 1] + cftab[i - 1];
    }

    // Validate cftab: last entry must equal nblock.
    if cftab[256] != nblock as i32 {
        return Err(fail(&format!(
            "cftab inconsistency: cftab[256]={} but nblock={nblock}",
            cftab[256]
        )));
    }

    // --- Compute the T^(-1) vector (fast variant) ---
    // For each byte in the block, compute the inverse BWT transform array.
    // tt[cftab[uc]] |= (i << 8), then cftab[uc]++.
    for i in 0..nblock {
        let uc = (tt[i] & 0xff) as usize;
        tt[cftab[uc] as usize] |= (i as u32) << 8;
        cftab[uc] += 1;
    }

    // --- BWT inverse transform output ---
    let mut t_pos = tt[orig_ptr as usize] >> 8;
    let mut nblock_used: usize = 0;

    // Read first byte.
    let mut k0 = bz_get_fast(&tt, &mut t_pos);
    nblock_used += 1;

    // RLE decode: bzip2 uses run-length encoding on the BWT output.
    // Runs of 1..4 identical bytes are stored literally; runs of 5+
    // are encoded as 4 copies followed by a repeat count byte.
    let mut state_out_len: i32 = 0;
    let mut state_out_ch: u8 = 0;

    while nblock_used <= nblock {
        if output.len() >= max_output {
            return Ok(());
        }

        if state_out_len > 0 {
            // Emit repeated byte.
            let to_emit = state_out_len as usize;
            let remaining = max_output - output.len();
            let emit_count = to_emit.min(remaining);
            for _ in 0..emit_count {
                output.push(state_out_ch);
            }
            state_out_len -= emit_count as i32;
            if state_out_len > 0 || output.len() >= max_output {
                return Ok(());
            }
            continue;
        }

        // state_out_len == 0: process the next run.
        state_out_ch = k0;
        // Count consecutive equal bytes (up to 4).
        let mut count = 1;
        // We need to peek at upcoming bytes to count the run.

        // First byte is k0, already consumed. Check for more.
        if nblock_used < nblock {
            k0 = bz_get_fast(&tt, &mut t_pos);
            nblock_used += 1;
            if k0 != state_out_ch {
                // Run of 1: emit and continue.
                output.push(state_out_ch);
                continue;
            }
            count = 2;

            if nblock_used < nblock {
                k0 = bz_get_fast(&tt, &mut t_pos);
                nblock_used += 1;
                if k0 != state_out_ch {
                    // Run of 2.
                    output.push(state_out_ch);
                    if output.len() < max_output {
                        output.push(state_out_ch);
                    }
                    continue;
                }
                count = 3;

                if nblock_used < nblock {
                    k0 = bz_get_fast(&tt, &mut t_pos);
                    nblock_used += 1;
                    if k0 != state_out_ch {
                        // Run of 3.
                        for _ in 0..3 {
                            if output.len() < max_output {
                                output.push(state_out_ch);
                            }
                        }
                        continue;
                    }
                    count = 4;

                    // After 4 identical bytes, the next byte is a repeat count.
                    if nblock_used < nblock {
                        k0 = bz_get_fast(&tt, &mut t_pos);
                        nblock_used += 1;
                        // k0 is the repeat count (0..255).
                        state_out_len = k0 as i32 + count;
                        // Fetch next k0 for the next iteration.
                        if nblock_used < nblock {
                            k0 = bz_get_fast(&tt, &mut t_pos);
                            nblock_used += 1;
                        }
                        continue;
                    }
                }
            }
        }

        // Emit whatever we collected at end of block.
        for _ in 0..count {
            if output.len() < max_output {
                output.push(state_out_ch);
            }
        }
        // k0 is exhausted at end of block; loop condition will exit.
    }

    // Flush any remaining repeated bytes.
    while state_out_len > 0 && output.len() < max_output {
        output.push(state_out_ch);
        state_out_len -= 1;
    }

    Ok(())
}

/// Reads one Huffman-coded symbol from the bitstream.
///
/// Equivalent to the `GET_MTF_VAL` macro in the C code: selects the
/// appropriate Huffman group based on the current position, then decodes
/// one symbol.
fn get_mtf_val(reader: &mut BitReader<'_>, huff: &mut HuffmanTables) -> Result<i32, Error> {
    // Advance to next group if needed.
    if huff.group_pos == 0 {
        huff.group_no += 1;
        if huff.group_no >= huff.n_selectors {
            return Err(fail("ran out of selectors"));
        }
        huff.group_pos = BZ_G_SIZE as i32;
    }
    huff.group_pos -= 1;

    let g_sel = huff.selector[huff.group_no as usize] as usize;
    let g_min_len = huff.min_lens[g_sel];
    let g_limit = &huff.limit[g_sel];
    let g_perm = &huff.perm[g_sel];
    let g_base = &huff.base[g_sel];

    let mut zn = g_min_len;
    let mut zvec = reader.get_bits(zn)?;

    loop {
        if zn > 20 {
            return Err(fail("Huffman code length exceeds 20"));
        }
        if zvec <= g_limit[zn as usize] {
            break;
        }
        zn += 1;
        let zj = reader.get_bit()?;
        zvec = (zvec << 1) | zj;
    }

    let idx = zvec - g_base[zn as usize];
    if idx < 0 || idx >= BZ_MAX_ALPHA_SIZE as i32 {
        return Err(fail("Huffman decoded index out of range"));
    }
    Ok(g_perm[idx as usize])
}

/// Performs MTF (Move-To-Front) decoding for a symbol.
///
/// Equivalent to the `uc = MTF(nextSym - 1)` block in the C code.
fn mtf_decode(
    next_sym: i32,
    mtfa: &mut [u8; MTFA_SIZE],
    mtfbase: &mut [usize; 256 / MTFL_SIZE],
) -> Result<u8, Error> {
    let nn = (next_sym - 1) as usize;

    if nn < MTFL_SIZE {
        // Fast path: symbol is in the first sub-list.
        let pp = mtfbase[0];
        let uc = mtfa[pp + nn];
        // Shift elements right by one.
        let mut pos = nn;
        while pos > 0 {
            mtfa[pp + pos] = mtfa[pp + pos - 1];
            pos -= 1;
        }
        mtfa[pp] = uc;
        Ok(uc)
    } else {
        // General case: symbol is in a later sub-list.
        let lno_init = nn / MTFL_SIZE;
        let off = nn % MTFL_SIZE;
        let mut pp = mtfbase[lno_init] + off;
        let uc = mtfa[pp];

        // Shift within the sub-list.
        while pp > mtfbase[lno_init] {
            mtfa[pp] = mtfa[pp - 1];
            pp -= 1;
        }
        mtfbase[lno_init] += 1;

        // Propagate across sub-lists.
        let mut lno = lno_init;
        while lno > 0 {
            mtfbase[lno] -= 1;
            mtfa[mtfbase[lno]] = mtfa[mtfbase[lno - 1] + MTFL_SIZE - 1];
            lno -= 1;
        }
        mtfbase[0] -= 1;
        mtfa[mtfbase[0]] = uc;

        // If mtfbase[0] hits 0, re-compact the MTF array.
        if mtfbase[0] == 0 {
            let mut kk = MTFA_SIZE - 1;
            for ii in (0..(256 / MTFL_SIZE)).rev() {
                for jj in (0..MTFL_SIZE).rev() {
                    mtfa[kk] = mtfa[mtfbase[ii] + jj];
                    kk = kk.wrapping_sub(1);
                }
                mtfbase[ii] = kk.wrapping_add(1);
            }
        }

        Ok(uc)
    }
}

/// Helper to create a `DecompressionFailed` error for bzip2.
fn fail(detail: &str) -> Error {
    Error::DecompressionFailed {
        method: "bzip2",
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_fails() {
        let result = decompress_bzip2(&[], DecodeLimit::Capped(1024));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_block_header_fails() {
        // 0xFF is not a valid block header byte.
        let result = decompress_bzip2(&[0xFF], DecodeLimit::Capped(1024));
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            Error::DecompressionFailed { method, detail } => {
                assert_eq!(method, "bzip2");
                assert!(detail.contains("invalid block header"));
            }
            _ => panic!("expected DecompressionFailed"),
        }
    }

    #[test]
    fn end_of_stream_produces_empty() {
        // 0x17 = end of stream immediately.
        let result = decompress_bzip2(&[0x17], DecodeLimit::Capped(1024));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn create_decode_tables_basic() {
        // Smoke test: 3 symbols with lengths [2, 1, 2].
        let length = [2u8, 1, 2];
        let mut limit_arr = [0i32; BZ_MAX_ALPHA_SIZE];
        let mut base_arr = [0i32; BZ_MAX_ALPHA_SIZE];
        let mut perm_arr = [0i32; BZ_MAX_ALPHA_SIZE];

        create_decode_tables(
            &mut limit_arr,
            &mut base_arr,
            &mut perm_arr,
            &length,
            1,
            2,
            3,
        );

        // perm should be: symbol 1 (length 1), then symbol 0 (length 2),
        // then symbol 2 (length 2).
        assert_eq!(perm_arr[0], 1);
        assert_eq!(perm_arr[1], 0);
        assert_eq!(perm_arr[2], 2);
    }

    #[test]
    fn bit_reader_reads_bits() {
        let data = [0b10110000, 0b01010000];
        let mut r = BitReader::new(&data);
        assert_eq!(r.get_bits(4).unwrap(), 0b1011);
        assert_eq!(r.get_bits(4).unwrap(), 0b0000);
        assert_eq!(r.get_bits(1).unwrap(), 0);
        assert_eq!(r.get_bits(1).unwrap(), 1);
        assert_eq!(r.get_bits(1).unwrap(), 0);
        assert_eq!(r.get_bits(1).unwrap(), 1);
    }

    #[test]
    fn bit_reader_eof() {
        let data = [0xFF];
        let mut r = BitReader::new(&data);
        assert!(r.get_bits(8).is_ok());
        assert!(r.get_bits(1).is_err());
    }
}
