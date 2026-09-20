//! Unresolved member access diagnostics (opt-in).
//!
//! Walk the precomputed [`SymbolMap`] for a file and flag every
//! `MemberAccess` span whose **subject type** cannot answer for the
//! member: either it is `mixed`, or it could not be worked out at all.
//! The message distinguishes the two, since a `mixed` subject is an
//! annotation the codebase never wrote while an unresolved one is a gap
//! in PHPantom's own inference. This is different from the
//! `unknown_members` diagnostic, which fires when the subject resolves
//! to a class but the specific member is missing.
//!
//! This diagnostic is **off by default** because most PHP codebases
//! lack comprehensive type annotations, which means PHPantom cannot
//! infer a type for many variables. Enabling this diagnostic on such
//! a codebase would flood the editor with noise.
//!
//! Enable it by adding the following to `.phpantom.toml`:
//!
//! ```toml
//! [diagnostics]
//! unresolved-member-access = true
//! ```
//!
//! The diagnostic uses `Severity::HINT` (not warning) because the
//! code is almost certainly correct. The purpose is to surface gaps
//! in type coverage so the developer can add annotations or discover
//! places where PHPantom's inference falls short.
//!
//! The diagnostic collection logic lives in the `Untyped` arm of
//! [`collect_unknown_member_diagnostics`](super::unknown_members) in
//! `unknown_members.rs`, since it shares the subject-type walk with the
//! unknown-member check. This module only re-exports the diagnostic
//! code constant.

/// Diagnostic code used for unresolved-member-access diagnostics.
pub(crate) const UNRESOLVED_MEMBER_ACCESS_CODE: &str = "unresolved_member_access";
