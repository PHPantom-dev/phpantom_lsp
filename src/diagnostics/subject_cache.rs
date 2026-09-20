//! Cache key for per-pass member-access subject resolution.
//!
//! A single file can contain hundreds of member-access spans that share
//! the same subject text (e.g. 60 occurrences of `$this->assertEquals`,
//! `$this->assertTrue`, …).  Without caching, each span triggers the
//! full resolution pipeline including `resolve_variable_types`, which
//! re-parses the entire file via `with_parsed_program`.
//!
//! Every diagnostic that caches subject resolutions must key that cache
//! the same way, because the key is what decides whether two accesses
//! are guaranteed to see the same type.  A key that is too coarse leaks
//! one access's type into another: keying only by `(variable_name,
//! class_name)` reports a deprecation against a same-named parameter in
//! a *different* method of the same class.  [`SubjectCacheKey::build`]
//! is the one place that decision lives, so all consumers share it.
//!
//! The key deliberately omits per-access byte offsets so the cache stays
//! effective — a service file with 200 accesses to `$model->` resolves
//! the variable once, not 200 times.  Expression-level narrowing
//! (ternary `instanceof`, inline `&&` chains) can refine a type at a
//! single byte offset without creating a narrowing block; consumers that
//! care handle it with an uncached re-resolution fallback rather than by
//! making the key finer.

use crate::symbol_map::SymbolMap;
use crate::types::{AccessKind, ClassInfo};

/// Scope identifier for the subject resolution cache.
///
/// Two member accesses share the same scope when they are inside the
/// same class body (identified by class name and byte offset of the
/// opening brace) **and** the same function/method/closure body
/// (identified by its start offset).  This prevents two methods in
/// the same class from sharing a cache entry when a same-named
/// variable has a different type in each method.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ScopeKey {
    /// Inside a class at the given byte offset, within a specific
    /// function/method/closure scope.  `fn_scope_start` is the byte
    /// offset of the enclosing function body (from
    /// [`SymbolMap::find_enclosing_scope`]), or `0` for class-level
    /// code outside any method.
    Class {
        name: String,
        start_offset: u32,
        fn_scope_start: u32,
    },
    /// Top-level code outside any class, within a specific
    /// function scope (`0` when truly top-level).
    TopLevel { fn_scope_start: u32 },
}

/// Cache key combining the subject text, access kind, and scope.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SubjectCacheKey {
    subject_text: String,
    access_kind: AccessKind,
    scope: ScopeKey,
    /// The `effective_from` offset of the active variable definition at
    /// the point of access, or `0` for non-variable subjects.  This
    /// ensures that accesses before and after a reassignment get
    /// separate cache entries.
    var_def_offset: u32,
    /// The innermost narrowing block containing the access for variable
    /// subjects, or `0` for non-variable subjects.
    /// This ensures that accesses inside different instanceof-narrowing
    /// contexts (e.g. different if-bodies) get independent cache
    /// entries.  Without this, the first access caches a narrowed type
    /// and subsequent accesses in a different narrowing context reuse
    /// the wrong result.
    narrowing_offset: u32,
    /// The offset of the most recent `assert($var instanceof …)`
    /// statement preceding this access, or `0` if there is none.
    /// Assert-instanceof statements act as sequential narrowing
    /// boundaries: they change the variable's resolved type without
    /// creating a block scope, so accesses before and after the
    /// assert must get separate cache entries.
    assert_offset: u32,
}

impl SubjectCacheKey {
    /// Build the cache key for a member access on `subject_text` at
    /// `access_offset`.
    pub(crate) fn build(
        symbol_map: &SymbolMap,
        current_class: Option<&ClassInfo>,
        subject_text: &str,
        access_kind: AccessKind,
        access_offset: u32,
    ) -> Self {
        let fn_scope_start = symbol_map.find_enclosing_scope(access_offset);

        // For variable subjects (excluding $this), compute the active
        // definition offset so that accesses before and after a
        // reassignment get separate cache entries.
        let var_def_offset = if subject_text.starts_with('$')
            && subject_text != "$this"
            && !subject_text.starts_with("$this->")
        {
            // Extract the bare variable name (e.g. "$file" from "$file"
            // or from a chain like "$file->foo()").  A null-safe hop is
            // already normalised to `->` by the subject-text lowering,
            // so there is no `?` to strip here.
            let var_name = subject_text
                .find("->")
                .map(|i| &subject_text[..i])
                .unwrap_or(subject_text);
            symbol_map.active_var_def_offset(
                &var_name[1..], // strip leading '$'
                access_offset,
            )
        } else {
            0
        };

        // Narrowing discrimination applies to every variable subject:
        // regular variables (`$var`), property chains (`$this->prop`), and
        // bare `$this` itself.  `if ($this instanceof GenericObjectType)`
        // narrows `$this` exactly as an `instanceof` on a parameter
        // narrows that, so a cache entry shared across the branch
        // boundary hands the branch the class the method was declared on
        // and reports every subclass member missing.
        let needs_narrowing_discriminator = subject_text.starts_with('$');
        let (narrowing_offset, assert_offset) = if needs_narrowing_discriminator {
            (
                symbol_map.find_narrowing_block(access_offset),
                symbol_map.find_preceding_assert_offset(access_offset),
            )
        } else {
            (0, 0)
        };

        SubjectCacheKey {
            subject_text: subject_text.to_string(),
            access_kind,
            scope: scope_key_for(current_class, fn_scope_start),
            var_def_offset,
            narrowing_offset,
            assert_offset,
        }
    }
}

/// Build a [`ScopeKey`] from the innermost enclosing class (if any)
/// and the enclosing function/method/closure scope start offset.
fn scope_key_for(current_class: Option<&ClassInfo>, fn_scope_start: u32) -> ScopeKey {
    match current_class {
        Some(cc) => ScopeKey::Class {
            name: cc.name.to_string(),
            start_offset: cc.start_offset,
            fn_scope_start,
        },
        None => ScopeKey::TopLevel { fn_scope_start },
    }
}
