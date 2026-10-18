//! The member accesses the unknown-member check judges, and the ones it
//! leaves alone: array callables, `@see` targets, `::class`, accesses a
//! `method_exists()` / `class_exists()` guard or `isset()` / `empty()`
//! protects, and a trait's accesses through its own `$this` / `self` /
//! `static` / `parent`.

use crate::class_lookup::find_class_at_offset;
use crate::symbol_map::{MappedSource, SymbolKind, SymbolSpan};
use crate::types::{AccessKind, ClassInfo, ClassLikeKind};

use super::super::existence_guards::ExistenceGuards;
use super::super::helpers::{ByteRange, is_offset_in_ranges};

/// The file-wide facts that decide which accesses are not checked.
pub(super) struct AccessFilters<'a> {
    pub(super) source: MappedSource<'a>,
    pub(super) local_classes: &'a [std::sync::Arc<ClassInfo>],
    pub(super) existence_guards: ExistenceGuards,
    /// Member/array access inside `isset()` / `empty()` never triggers a
    /// runtime error even when the accessed member doesn't exist.
    pub(super) isset_empty_ranges: Vec<ByteRange>,
}

/// One member access the check judges.
pub(super) struct MemberAccessSite<'a> {
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) subject_text: &'a str,
    pub(super) member_name: &'a str,
    pub(super) is_static: bool,
    pub(super) is_method_call: bool,
    pub(super) is_docblock_ref: bool,
    pub(super) is_nullsafe: bool,
    /// The class the access is written in.
    pub(super) current_class: Option<&'a ClassInfo>,
}

impl<'a> MemberAccessSite<'a> {
    /// The member access `span` is, unless it is not one or the check
    /// leaves it alone.
    pub(super) fn of_span(span: &'a SymbolSpan, filters: &AccessFilters<'a>) -> Option<Self> {
        let SymbolKind::MemberAccess {
            subject_text,
            member_name,
            is_static,
            is_method_call,
            docblock_ref,
            is_array_callable,
            is_nullsafe,
        } = &span.kind
        else {
            return None;
        };
        // A `[Class::class, 'method']` / `[$obj, 'method']` array literal
        // is only a callable when it flows into a callable-typed context
        // (a callable parameter, `is_callable`, or a direct invocation).
        // Without type-flow analysis we cannot tell such an array apart
        // from a plain data pair like `return [[Foo::class, 'name'], ...]`,
        // so we do not validate its second element as a method.  The span
        // still drives navigation, hover, and semantic tokens, which only
        // surface information when the member actually resolves.
        if *is_array_callable {
            return None;
        }
        // `@see` legally carries URIs, prose, and naming suggestions in
        // addition to FQSENs, so a target that resolves to nothing is
        // not an error.  PHPUnit coverage metadata, which shares the
        // emitter, is still checked: it names a code unit that must
        // exist.
        if docblock_ref.tolerates_missing_target() {
            return None;
        }

        let subject_text = subject_text.as_str(filters.source);
        let member_name = member_name.as_str();
        let is_static = *is_static;

        // ── Skip the magic `::class` constant ───────────────────
        if is_static && member_name.eq_ignore_ascii_case("class") {
            return None;
        }

        // ── Skip members guarded by method_exists() ───────────────
        if filters
            .existence_guards
            .is_method_guarded(member_name, span.start)
        {
            return None;
        }

        // ── Skip accesses inside isset()/empty() ──────────────────
        // `isset($x->prop)` and `empty($x->prop)` never error or warn
        // even when `prop` doesn't exist — that is their purpose.
        if is_offset_in_ranges(span.start, &filters.isset_empty_ranges) {
            return None;
        }

        // ── Skip members on classes guarded by class_exists() ─────
        // When the subject is a static access on a class guarded by
        // class_exists(), suppress unknown member diagnostics too.  The
        // subject_text for static access is typically the class name.
        if is_static
            && filters
                .existence_guards
                .is_class_guarded(subject_text.trim(), span.start)
        {
            return None;
        }

        let current_class = find_class_at_offset(filters.local_classes, span.start);

        // ── Suppress inside traits for self-referencing subjects ────
        // Traits are incomplete: they expect host classes to provide
        // members accessed via $this/self/static/parent.  Flagging
        // these produces false positives for every trait that relies
        // on the host class's members.
        //
        // This also covers chain expressions rooted at these keywords,
        // e.g. `static::where('x', 'y')->update(...)` has subject_text
        // `"static::where('x', 'y')"` and `$this->relation()->first()`
        // has subject_text `"$this->relation()"`.  The root of the
        // chain is still the trait's self-reference, so the entire
        // chain is unsuppressable without knowing the host class.
        if let Some(cc) = current_class
            && cc.kind == ClassLikeKind::Trait
            && subject_text_is_rooted_in_self(subject_text)
        {
            return None;
        }

        Some(Self {
            start: span.start,
            end: span.end,
            subject_text,
            member_name,
            is_static,
            is_method_call: *is_method_call,
            is_docblock_ref: docblock_ref.is_reference(),
            is_nullsafe: *is_nullsafe,
            current_class,
        })
    }

    pub(super) fn access_kind(&self) -> AccessKind {
        AccessKind::of_member_access(self.is_static)
    }

    /// `$this`, `self`, and `static` name whatever class the code is
    /// bound to, which is normally the one it sits in — but a closure can
    /// be bound elsewhere, and then the class it binds to is the scope PHP
    /// checks visibility against.  Only the bare keywords qualify: in a
    /// chain like `$this->rel()->x` the receiver is whatever the chain
    /// returned, not the binding.
    pub(super) fn subject_binds_scope(&self) -> bool {
        matches!(self.subject_text, "$this" | "self" | "static")
    }

    /// Whether this subject could benefit from expression-level narrowing
    /// re-resolution.  Every subject rooted at a variable can:
    /// `$this->pair[0] instanceof Sub && !$this->pair[0]->flag()` refines
    /// the second occurrence with no block to key it on, and so does
    /// `$this instanceof Sub && $this->only()`.
    pub(super) fn is_narrowable_subject(&self) -> bool {
        self.subject_text.starts_with('$')
    }

    /// "method" or "property", for messages.
    pub(super) fn kind_label(&self) -> &'static str {
        if self.is_method_call {
            "method"
        } else {
            "property"
        }
    }
}

/// Check whether a subject text is rooted in `$this`, `self`, `static`,
/// or `parent`.  This matches both bare keywords (`"$this"`, `"static"`)
/// and chain expressions that start with one of them
/// (`"$this->relation()"`, `"static::where('x', 'y')"`, `"self::$prop"`).
fn subject_text_is_rooted_in_self(subject_text: &str) -> bool {
    // Bare keyword match (most common case).
    if matches!(subject_text, "$this" | "self" | "static" | "parent") {
        return true;
    }

    // Chain rooted at `$this->` or `$this?->`
    if subject_text.starts_with("$this->") || subject_text.starts_with("$this?->") {
        return true;
    }

    // Chain rooted at `self::`, `static::`, or `parent::`
    if subject_text.starts_with("self::")
        || subject_text.starts_with("static::")
        || subject_text.starts_with("parent::")
    {
        return true;
    }

    false
}
