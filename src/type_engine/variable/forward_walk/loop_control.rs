// ─── Forward walk entry point ───────────────────────────────────────────────

use std::cell::RefCell;

use mago_syntax::cst::{Expression, Literal};

use super::ScopeState;

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

/// One level of loop nesting, held for as long as the loop's body is
/// being walked.
///
/// The depth is restored when the guard drops, so a panic that unwinds
/// out of a loop body (request handlers catch it and the worker thread
/// lives on) does not leave the thread believing it is still inside the
/// loop, which would clamp every later walk's fixed-point passes and,
/// past [`MAX_LOOP_DEPTH`], skip every loop body on that thread.
pub(crate) struct LoopDepthGuard {
    depth: u32,
}

impl LoopDepthGuard {
    /// Enter one more level of loop nesting.
    pub(crate) fn enter() -> Self {
        let depth = LOOP_DEPTH.with(|c| {
            let v = c.get() + 1;
            c.set(v);
            v
        });
        Self { depth }
    }

    /// The nesting depth this guard entered (1 for an outermost loop).
    pub(crate) fn depth(&self) -> u32 {
        self.depth
    }
}

impl Drop for LoopDepthGuard {
    fn drop(&mut self) {
        LOOP_DEPTH.with(|c| c.set(self.depth - 1));
    }
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

// ─── Loop exit edges ────────────────────────────────────────────────────────

thread_local! {
    /// One frame per enclosing breakable structure (loop or `switch`),
    /// innermost last.  A `break`/`continue` writes the scope it leaves
    /// with into the frame its level names, and the structure that owns
    /// the frame folds those states into its own join.
    ///
    /// Set and cleared within a single synchronous walk, like
    /// [`LOOP_DEPTH`] above.
    static EXIT_EDGES: RefCell<Vec<ExitEdges>> = const { RefCell::new(Vec::new()) };
}

/// The scope states that jump out of one breakable structure.
#[derive(Default)]
pub(crate) struct ExitEdges {
    /// States that left the structure entirely (`break`).  They join the
    /// code *after* it.
    pub breaks: Vec<ScopeState>,
    /// States that skipped to the next iteration (`continue`).  They join
    /// the end of the loop body, which is what the back edge and the
    /// exit condition both see.
    pub continues: Vec<ScopeState>,
}

/// An open exit-edge frame for a loop or `switch` whose body is being
/// walked.
///
/// [`Self::pop`] closes it and hands back what jumped out; a guard that is
/// dropped without being popped (a panic unwinding out of the body) still
/// closes its frame, so the frames of the structures around it stay
/// aligned with the walk that owns them.
#[must_use]
pub(crate) struct ExitFrameGuard {
    popped: bool,
}

impl ExitFrameGuard {
    /// Open an exit-edge frame for a body about to be walked.
    pub(crate) fn push() -> Self {
        EXIT_EDGES.with(|frames| frames.borrow_mut().push(ExitEdges::default()));
        Self { popped: false }
    }

    /// Close the frame and return what jumped out of it.
    pub(crate) fn pop(mut self) -> ExitEdges {
        self.popped = true;
        EXIT_EDGES.with(|frames| frames.borrow_mut().pop().unwrap_or_default())
    }
}

impl Drop for ExitFrameGuard {
    fn drop(&mut self) {
        if self.popped {
            return;
        }
        // Unwinding through a borrow (a panic inside `record_exit_edge`)
        // must not become a second panic here.
        EXIT_EDGES.with(|frames| {
            if let Ok(mut frames) = frames.try_borrow_mut() {
                frames.pop();
            }
        });
    }
}

/// Discard the edges recorded so far by the innermost frame.
///
/// A loop body is walked several times to propagate loop-carried types;
/// only the last walk's edges describe the loop, so each walk starts from
/// a clean frame.
pub(crate) fn clear_exit_frame() {
    EXIT_EDGES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            frame.breaks.clear();
            frame.continues.clear();
        }
    });
}

/// Fold the innermost frame's `continue` states into `scope` and remove
/// them.
///
/// A `continue` rejoins the loop at the end of the body, so its state is
/// an alternative to the fall-through both for the next iteration and for
/// the exit condition that follows.
pub(crate) fn drain_continue_edges(scope: &mut ScopeState) {
    let edges = EXIT_EDGES.with(|frames| {
        frames
            .borrow_mut()
            .last_mut()
            .map(|frame| std::mem::take(&mut frame.continues))
            .unwrap_or_default()
    });
    merge_exit_edges(scope, &edges);
}

/// Record the state a `break` or `continue` carries out of the structure
/// its `level` names (1 = innermost, the default).
///
/// A level naming more structures than are open (or one written as
/// something other than an integer literal) is attributed to the
/// outermost open frame, which is the closest reachable approximation.
pub(crate) fn record_exit_edge(level: Option<u32>, is_break: bool, scope: &ScopeState) {
    if scope.unreachable {
        return;
    }
    EXIT_EDGES.with(|frames| {
        let mut frames = frames.borrow_mut();
        let depth = frames.len();
        if depth == 0 {
            return;
        }
        let level = level.unwrap_or(1).max(1) as usize;
        let index = depth.saturating_sub(level);
        let frame = &mut frames[index];
        if is_break {
            frame.breaks.push(scope.clone());
        } else {
            frame.continues.push(scope.clone());
        }
    });
}

/// The integer level of a `break`/`continue`, when it is written as a
/// literal.  `break;` and a computed level both yield `None`.
pub(crate) fn exit_level(level: Option<&Expression<'_>>) -> Option<u32> {
    match level? {
        Expression::Literal(Literal::Integer(lit)) => {
            crate::atom::bytes_to_str(lit.raw).parse().ok()
        }
        _ => None,
    }
}

/// Fold a set of exit states into `scope` as alternative incoming paths.
pub(crate) fn merge_exit_edges(scope: &mut ScopeState, edges: &[ScopeState]) {
    for edge in edges {
        scope.merge_branch(edge);
    }
}
