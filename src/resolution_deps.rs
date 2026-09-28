//! Records which class and function names a resolution consulted.
//!
//! The resolved-member layer (`ResolvedMemberFile` in
//! [`reference_index`](crate::reference_index)) caches, per candidate file,
//! the receiver class of every member access a reference search resolved.
//! Rebuilding one entry runs the type engine over a whole file, so throwing
//! the layer away on every signature keystroke is what makes a rename on a
//! large project pay the full workspace scan again.
//!
//! An entry can only be kept across an edit if the edit cannot change what
//! it resolved to, and that is not decidable from the names the file
//! mentions: a chain such as `$b->makeC()->handle()` reaches its receiver
//! through classes the file never names. So the resolution itself reports
//! what it looked at. Every class lookup funnels through
//! [`Backend::find_or_load_class_typed`] and every function lookup through
//! [`Backend::find_or_load_function`], so a recorder activated around one
//! file's resolution sees the whole dependency set, including the lookups
//! that found nothing (a name that later gains a declaration changes the
//! answer too).
//!
//! [`Backend::find_or_load_class_typed`]: crate::Backend::find_or_load_class_typed
//! [`Backend::find_or_load_function`]: crate::Backend::find_or_load_function
//!
//! # Keys
//!
//! Names are keyed by their last segment, ASCII-lowercased. A lookup asks
//! under whatever spelling the type engine had in hand (`Foo`, `App\Foo`, an
//! import alias), while an edit reports the fully-qualified names it
//! changed, and the two only reliably agree on the short name. Keying on it
//! over-approximates — an edit to `App\Foo` also drops entries that
//! consulted `Other\Foo` — which costs a recomputation, never a wrong
//! answer.
//!
//! # Not a re-entry guard
//!
//! The thread-local is a collection point, not a gate: it never changes what
//! a lookup returns. It is set and read within one synchronous call tree, so
//! it cannot be resumed on another thread mid-recording, and a nested
//! recording merges its names into the enclosing one on drop rather than
//! hiding them.

use std::cell::RefCell;

use crate::atom::{Atom, AtomSet, ascii_lowercase_atom};
use crate::util::short_name;

thread_local! {
    /// The set being filled, or `None` when this thread is not recording.
    static CONSULTED: RefCell<Option<AtomSet>> = const { RefCell::new(None) };
}

/// The key a symbol name is recorded and looked up under.
#[inline]
pub(crate) fn dep_key(name: &str) -> Atom {
    ascii_lowercase_atom(short_name(name))
}

/// Note that `name` was consulted, if this thread is recording.
#[inline]
pub(crate) fn record(name: &str) {
    CONSULTED.with(|cell| {
        let Ok(mut recording) = cell.try_borrow_mut() else {
            return;
        };
        if let Some(consulted) = recording.as_mut() {
            consulted.insert(dep_key(name));
        }
    });
}

/// [`record`] for a whole candidate list, reaching the thread-local once
/// rather than once per name: function resolution tries several spellings and
/// runs on the hottest path the type engine has.
#[inline]
pub(crate) fn record_all(names: &[&str]) {
    CONSULTED.with(|cell| {
        let Ok(mut recording) = cell.try_borrow_mut() else {
            return;
        };
        if let Some(consulted) = recording.as_mut() {
            consulted.extend(names.iter().map(|name| dep_key(name)));
        }
    });
}

/// An active recording. Names consulted on this thread are collected until
/// it is dropped.
pub(crate) struct Recording {
    /// The recording this one suspended, restored on drop.
    outer: Option<AtomSet>,
}

/// Start recording on this thread.
pub(crate) fn record_consulted_names() -> Recording {
    Recording {
        outer: CONSULTED.with(|cell| cell.borrow_mut().replace(AtomSet::default())),
    }
}

impl Recording {
    /// The names consulted so far, in arbitrary order.
    pub(crate) fn consulted(&self) -> Vec<Atom> {
        CONSULTED.with(|cell| {
            cell.borrow()
                .as_ref()
                .map(|consulted| consulted.iter().copied().collect())
                .unwrap_or_default()
        })
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        CONSULTED.with(|cell| {
            let mut slot = cell.borrow_mut();
            let inner = slot.take();
            let mut outer = self.outer.take();
            if let (Some(outer), Some(inner)) = (outer.as_mut(), inner) {
                outer.extend(inner);
            }
            *slot = outer;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut names: Vec<Atom>) -> Vec<String> {
        names.sort_unstable();
        names.into_iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn nothing_is_recorded_without_an_active_recording() {
        record("App\\Foo");
        let recording = record_consulted_names();
        assert!(recording.consulted().is_empty());
    }

    #[test]
    fn names_are_keyed_by_case_folded_short_name() {
        let recording = record_consulted_names();
        record("App\\Models\\User");
        record("user");
        record("\\Other\\USER");
        assert_eq!(sorted(recording.consulted()), vec!["user".to_string()]);
    }

    #[test]
    fn a_nested_recording_merges_into_the_one_it_suspended() {
        let outer = record_consulted_names();
        record("Outer");
        {
            let inner = record_consulted_names();
            record("Inner");
            assert_eq!(sorted(inner.consulted()), vec!["inner".to_string()]);
        }
        record("After");
        assert_eq!(
            sorted(outer.consulted()),
            vec![
                "after".to_string(),
                "inner".to_string(),
                "outer".to_string()
            ]
        );
    }
}
