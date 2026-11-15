//! Per-thread counts of per-file source work, compiled only into unit tests.
//!
//! `collect` binds evidence to exact source bytes, and its cost is dominated by
//! how often each selected file is read, hashed, decoded and parsed. A unit test
//! asserts those counts directly instead of inferring them from timings. The
//! module and every call site are `#[cfg(test)]`, so no build outside unit
//! tests carries a counter.

use std::cell::Cell;

/// One unit of per-file source work.
#[derive(Clone, Copy)]
pub(crate) enum Unit {
    /// A full read of a selected source file.
    Read = 0,
    /// A SHA-256 pass over source bytes.
    Hash = 1,
    /// A decode of source bytes to text.
    Decode = 2,
    /// A tree-sitter parse of decoded source.
    Parse = 3,
}

thread_local! {
    static COUNTS: Cell<[u64; 4]> = const { Cell::new([0; 4]) };
}

/// Record one unit of work on the current thread.
pub(crate) fn count(unit: Unit) {
    COUNTS.with(|c| {
        let mut counts = c.get();
        counts[unit as usize] += 1;
        c.set(counts);
    });
}

/// Return and reset this thread's counts as `[reads, hashes, decodes, parses]`.
pub(crate) fn take() -> [u64; 4] {
    COUNTS.with(|c| c.replace([0; 4]))
}

/// Add counts gathered on another thread, such as a finished scan worker.
pub(crate) fn add(counts: [u64; 4]) {
    COUNTS.with(|c| {
        let mut total = c.get();
        for (sum, n) in total.iter_mut().zip(counts) {
            *sum += n;
        }
        c.set(total);
    });
}
