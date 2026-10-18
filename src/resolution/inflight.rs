//! Making a lookup that needs a file another thread is already parsing
//! wait for that parse instead of starting its own.

use std::collections::HashMap;

/// Deduplicates concurrent parses of the same file.
///
/// The first thread to request a URI claims it and performs the parse;
/// any other thread that requests the same URI while the parse is in
/// flight blocks on a condvar until the claim is released, then reads
/// the completed result from `uri_classes_index`.
///
/// Blocking (rather than spin-waiting with a timeout) matters for
/// correctness: a timed-out waiter would conclude the class does not
/// exist and poison the `class_not_found_cache` for the rest of the
/// process, turning a scheduling hiccup into permanently unresolved
/// types.  Waiting is always finite because a parse never parses
/// another file (so there are no claim cycles) and the claim is
/// released via an RAII guard even when the parse unwinds.
///
/// Claims are keyed by URI and record the owning thread, so a
/// (currently impossible) re-entrant parse of the same URI on the same
/// thread degrades to reading the current cached state instead of
/// self-deadlocking.
pub(crate) struct ParseInflight {
    /// URI → thread currently parsing it.
    entries: parking_lot::Mutex<HashMap<String, std::thread::ThreadId>>,
    /// Notified whenever a claim is released.
    released: parking_lot::Condvar,
}

impl ParseInflight {
    pub(crate) fn new() -> Self {
        Self {
            entries: parking_lot::Mutex::new(HashMap::new()),
            released: parking_lot::Condvar::new(),
        }
    }

    /// Try to claim `uri` for parsing on the current thread.
    ///
    /// Returns `true` when the claim was acquired (the caller must
    /// parse and then release via [`InflightGuard`]), `false` when
    /// another thread already holds the claim.
    pub(super) fn try_claim(&self, uri: &str) -> bool {
        let mut entries = self.entries.lock();
        match entries.get(uri) {
            Some(_) => false,
            None => {
                entries.insert(uri.to_string(), std::thread::current().id());
                true
            }
        }
    }

    /// Block until no thread holds a claim on `uri`.
    ///
    /// If the current thread itself holds the claim (re-entrant parse),
    /// returns immediately instead of deadlocking — the caller then
    /// sees whatever partial state is already cached.
    ///
    /// A single parse takes milliseconds, so the 10-second escape
    /// hatch is never reached in normal operation.  It exists so that
    /// if a parse ever hangs abnormally (e.g. a future caller waiting
    /// while holding a lock the parsing thread needs), the waiter
    /// degrades to a loudly-logged stale read instead of hanging the
    /// analyzer forever.
    pub(super) fn wait_until_released(&self, uri: &str) {
        const ESCAPE_HATCH: std::time::Duration = std::time::Duration::from_secs(10);
        let deadline = std::time::Instant::now() + ESCAPE_HATCH;
        let mut entries = self.entries.lock();
        while let Some(owner) = entries.get(uri) {
            if *owner == std::thread::current().id() {
                tracing::warn!(
                    "PHPantom: re-entrant parse of {uri} on the same thread; \
                     returning current cached state"
                );
                return;
            }
            if self.released.wait_until(&mut entries, deadline).timed_out() {
                tracing::warn!(
                    "PHPantom: gave up waiting for another thread's parse of {uri} \
                     after {ESCAPE_HATCH:?}; results for this file may be incomplete"
                );
                return;
            }
        }
    }

    /// Release the claim on `uri` and wake all waiters.
    fn release(&self, uri: &str) {
        self.entries.lock().remove(uri);
        self.released.notify_all();
    }
}

/// RAII guard that releases a URI claim in `parse_inflight` on drop.
///
/// Holding the claim in a guard ensures that if parsing or extraction
/// unwinds (panics), the claim is still released and waiting threads
/// wake up. Without this, a panic would leave the URI claimed forever,
/// blocking every subsequent lookup of the same file.
pub(super) struct InflightGuard<'a> {
    /// The shared inflight set the URI was claimed in.
    pub(super) inflight: &'a ParseInflight,
    /// The URI to release on drop.
    pub(super) uri: String,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        self.inflight.release(&self.uri);
    }
}

#[cfg(test)]
mod tests {
    //! The inflight guard must release its URI claim in `parse_inflight`
    //! even when the parse/extraction work unwinds, so a panic can never
    //! leave a URI claimed forever (blocking every subsequent lookup).
    //! Waiters must block until the claim is released — never time out
    //! into a "class not found" conclusion — and a re-entrant wait on
    //! the claiming thread must return instead of self-deadlocking.

    use super::{InflightGuard, ParseInflight};

    #[test]
    fn guard_releases_claim_on_panic() {
        let inflight = ParseInflight::new();
        assert!(inflight.try_claim("file:///a.php"));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = InflightGuard {
                inflight: &inflight,
                uri: "file:///a.php".to_string(),
            };
            panic!("boom");
        }));

        assert!(
            result.is_err(),
            "the panic should propagate to catch_unwind"
        );
        assert!(
            inflight.try_claim("file:///a.php"),
            "guard must release the claim even when the scope unwinds"
        );
    }

    #[test]
    fn guard_releases_claim_on_normal_drop() {
        let inflight = ParseInflight::new();
        assert!(inflight.try_claim("file:///b.php"));
        {
            let _guard = InflightGuard {
                inflight: &inflight,
                uri: "file:///b.php".to_string(),
            };
        }
        assert!(
            inflight.try_claim("file:///b.php"),
            "guard must release the claim when it drops normally"
        );
    }

    #[test]
    fn waiter_blocks_until_claim_released() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let inflight = ParseInflight::new();
        assert!(inflight.try_claim("file:///c.php"));
        let released = AtomicBool::new(false);

        std::thread::scope(|s| {
            let waiter = s.spawn(|| {
                inflight.wait_until_released("file:///c.php");
                assert!(
                    released.load(Ordering::SeqCst),
                    "waiter must not wake up before the claim is released"
                );
            });

            // Hold the claim long enough that a woken-too-early waiter
            // would observe `released == false` and fail the assert.
            std::thread::sleep(std::time::Duration::from_millis(50));
            released.store(true, Ordering::SeqCst);
            inflight.release("file:///c.php");
            waiter.join().unwrap();
        });
    }

    #[test]
    fn reentrant_wait_on_claiming_thread_returns() {
        let inflight = ParseInflight::new();
        assert!(inflight.try_claim("file:///d.php"));
        // Same thread holds the claim — must return, not deadlock.
        inflight.wait_until_released("file:///d.php");
        inflight.release("file:///d.php");
    }

    #[test]
    fn second_claim_fails_while_held() {
        let inflight = ParseInflight::new();
        assert!(inflight.try_claim("file:///e.php"));
        std::thread::scope(|s| {
            s.spawn(|| {
                assert!(
                    !inflight.try_claim("file:///e.php"),
                    "a held claim must not be claimable from another thread"
                );
            })
            .join()
            .unwrap();
        });
    }
}
