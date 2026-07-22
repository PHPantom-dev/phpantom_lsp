// ─── Forward walk entry point ───────────────────────────────────────────────

thread_local! {
    /// Tracks the current loop nesting depth (foreach, while, for,
    /// do-while).  Used to reduce the number of loop iterations for
    /// deeply nested loops, preventing the exponential blowup that
    /// occurs when loop iteration interacts with if-branch merging.
    static LOOP_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Maximum loop nesting depth before loop bodies are skipped entirely.
/// PHP code rarely nests loops beyond 6 levels; this is a hard safety net.
pub(crate) const MAX_LOOP_DEPTH: u32 = 6;

/// Increment the loop depth counter and return the new depth.
pub(crate) fn enter_loop() -> u32 {
    LOOP_DEPTH.with(|c| {
        let v = c.get() + 1;
        c.set(v);
        v
    })
}

/// Decrement the loop depth counter.
pub(crate) fn leave_loop(depth: u32) {
    LOOP_DEPTH.with(|c| c.set(depth - 1));
}

/// Clamp `max_iterations` based on the current loop nesting depth.
///
/// At depth 1 (outermost loop), the full assignment-depth-bounded
/// iteration count is used.  At depth 2, cap at 2 iterations.
/// At depth 3+, use a single pass only.  This prevents exponential
/// blowup from the interaction of loop iteration with if-branch
/// merging in deeply nested loops.
pub(crate) fn clamp_iterations_for_depth(max_iterations: u32, loop_depth: u32) -> u32 {
    match loop_depth {
        0 | 1 => max_iterations,
        2 => max_iterations.min(2),
        _ => 1,
    }
}
