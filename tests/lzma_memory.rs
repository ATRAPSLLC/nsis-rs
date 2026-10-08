//! Peak memory of an LZMA decode, measured with a counting allocator.
//!
//! The LZMA header declares a dictionary size and the decoder allocates all of
//! it up front, so the declared size - not the output budget - would bound
//! memory if `decompress_lzma` passed it through. This lives in its own test
//! binary because the allocator is process-wide.

#![allow(unsafe_code, clippy::unwrap_used, clippy::arithmetic_side_effects)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use nsis::decompress::{DecodeLimit, lzma::decompress_lzma};

/// Forwards to [`System`] and records the most memory held at once.
struct PeakAlloc;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call forwards to `System` with the caller's arguments
// unchanged; the counters only observe sizes.
unsafe impl GlobalAlloc for PeakAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            let now = CURRENT.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
            let now = CURRENT.fetch_add(new_size, Ordering::Relaxed) + new_size;
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: PeakAlloc = PeakAlloc;

#[test]
fn declared_dictionary_does_not_outgrow_the_budget() {
    const BUDGET: usize = 1024 * 1024;

    // Properties 0x5D, a dictionary of 0xFFFF_FFF0 (the largest lzma-rust2
    // accepts, just under 4 GiB), then garbage that fails to decode.
    let mut stream = vec![0x5D];
    stream.extend_from_slice(&0xFFFF_FFF0_u32.to_le_bytes());
    stream.extend_from_slice(&[0; 64]);

    let before = CURRENT.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let result = decompress_lzma(&stream, DecodeLimit::Capped(BUDGET));
    let used = PEAK.load(Ordering::Relaxed) - before;

    assert!(result.is_err(), "the garbage body must not decode");
    assert!(
        used <= 2 * BUDGET,
        "decoding under a {BUDGET}-byte budget held {used} bytes at once"
    );
}
