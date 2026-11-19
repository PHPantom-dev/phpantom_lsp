//! The member accesses the unknown-member check judges, and the ones it
//! leaves alone: array callables, `@see` targets, `::class`, accesses a
//! `method_exists()` / `class_exists()` guard or `isset()` / `empty()`
//! protects, and a trait's accesses through its own `$this` / `self` /
//! `static` / `parent`, or through a chain from them that leads back to
//! the trait or to nothing known.

use crate::class_lookup::find_class_at_offset;
use crate::symbol_map::{DocblockMemberRef, MappedSource, SymbolKind, SymbolSpan};
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
    /// Where the method names of the array callables known to be called
    /// start, in order: the listeners of a Yii events file.
    pub(super) called_array_callables: Vec<u32>,
    /// The callbacks the file's Drupal form and render arrays name.
    pub(super) drupal_callbacks: &'a [crate::symbol_map::DrupalCallbackSite],
}

/// One member access the check judges.
pub(super) struct MemberAccessSite<'a> {
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) subject_text: &'a str,
    pub(super) member_name: &'a str,
    pub(super) is_static: bool,
    pub(super) is_method_call: bool,
    /// Whether a static access names a property (`Foo::$bar`) rather
    /// than a constant (`Foo::BAR`).
    pub(super) is_static_property: bool,
    pub(super) is_docblock_ref: bool,
    /// Whether any member of the name will do, as for a docblock
    /// reference, rather than only one of the kind the access asks for.
    pub(super) relaxed_lookup: bool,
    pub(super) is_nullsafe: bool,
    /// Whether the member is the method a mock expectation names, which
    /// is checked on the mocked class (see [`crate::mock_methods`]).
    pub(super) mock_expectation: bool,
    /// Whether the member is a field a Doctrine repository call names,
    /// which is checked against the entity's mapping rather than looked up
    /// on the subject (see [`crate::doctrine`]).
    pub(super) entity_field: bool,
    /// Whether the member is an attribute a Yii Active Record call names,
    /// which is checked on the records among the subject's classes only
    /// (see [`crate::yii::active_record`]).
    pub(super) yii_attribute: bool,
    /// Whether the member is a property a Yii form field or form hydrator
    /// names, which is checked only on a form model (see
    /// [`crate::yii::forms`]).
    pub(super) form_model_property: bool,
    /// The class the access is written in.
    pub(super) current_class: Option<&'a ClassInfo>,
    /// Whether the access is in a trait and its subject is a chain rooted
    /// at `$this`, `self`, `static` or `parent`.  Such a chain is checked
    /// only when it resolves to something other than the trait: a member
    /// the chain lands back on the trait for is one the host provides.
    pub(super) trait_self_chain: bool,
    /// Whether the member is the method a Drupal callback names, which
    /// Drupal calls from outside every class.
    pub(super) drupal_callback: bool,
    /// Whether the member is the method a Drupal form callback's
    /// `'::method'` names, which is checked only when the class it is
    /// written in is a form (see [`crate::drupal::forms`]).
    pub(super) form_callback: bool,
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
        // so we do not validate its second element as a method, unless the
        // file is known to call it, as Drupal calls the callbacks its form
        // and render arrays name.  The span still drives navigation,
        // hover, and semantic tokens, which only surface information when
        // the member actually resolves.
        let drupal_callback = *is_array_callable
            && crate::drupal::forms::callback_at(filters.drupal_callbacks, span.start)
                .is_some_and(crate::drupal::forms::looks_up_method);
        if *is_array_callable
            && !drupal_callback
            && filters
                .called_array_callables
                .binary_search(&span.start)
                .is_err()
        {
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
        // A chain rooted at one of them (`$this->manager->x()`) is
        // different: its subject is whatever the chain resolved to, so
        // it is judged once that is known (see [`Self::trait_self_chain`]).
        let in_trait = current_class.is_some_and(|cc| cc.kind == ClassLikeKind::Trait);
        if in_trait && matches!(subject_text, "$this" | "self" | "static" | "parent") {
            return None;
        }
        let trait_self_chain = in_trait && subject_text_is_rooted_in_self(subject_text);
        // A form callback's `'::method'` is a call Drupal makes on the form,
        // not a name a docblock gives.
        let form_callback = *docblock_ref == DocblockMemberRef::FormCallback;
        let is_docblock_ref = docblock_ref.is_reference() && !form_callback;

        Some(Self {
            start: span.start,
            end: span.end,
            subject_text,
            member_name,
            is_static,
            is_method_call: *is_method_call,
            // The name is stored without the `$` that `Foo::$bar` and a
            // Yii definition's `'$bar'` key write just before it, or at
            // the start of the span.
            is_static_property: is_static
                && !*is_method_call
                && filters
                    .source
                    .text()
                    .as_bytes()
                    .get(span.start.saturating_sub(1) as usize..=span.start as usize)
                    .is_some_and(|bytes| bytes.contains(&b'$')),
            is_docblock_ref,
            // A Yii form field or hydration map reads or writes a property
            // by reflection, which a method or a constant cannot stand in
            // for, though its visibility does not matter.
            relaxed_lookup: is_docblock_ref
                && !matches!(docblock_ref, DocblockMemberRef::YiiProperty { .. }),
            is_nullsafe: *is_nullsafe,
            mock_expectation: *docblock_ref == DocblockMemberRef::MockedMethod,
            entity_field: *docblock_ref == DocblockMemberRef::EntityField,
            yii_attribute: *docblock_ref == DocblockMemberRef::YiiAttribute,
            form_model_property: *docblock_ref
                == DocblockMemberRef::YiiProperty { form_model: true },
            current_class,
            trait_self_chain,
            drupal_callback,
            form_callback,
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

    /// "method", "property", or "constant", for messages.
    pub(super) fn kind_label(&self) -> &'static str {
        if self.is_method_call {
            "method"
        } else if self.is_static && !self.is_static_property {
            "constant"
        } else {
            "property"
        }
    }

    /// [`Self::kind_label`] at the start of a message.
    pub(super) fn kind_title(&self) -> &'static str {
        match self.kind_label() {
            "method" => "Method",
            "constant" => "Constant",
            _ => "Property",
        }
    }

    /// The member as PHP spells it, on `owner` when there is one:
    /// `Foo::bar()`, `Foo::$bar`, `Foo::BAR`, or `bar()`, `$bar`, `BAR`.
    pub(super) fn spelled(&self, owner: Option<&str>) -> String {
        let member = if self.is_method_call {
            format!("{}()", self.member_name)
        } else if self.is_static && !self.is_static_property {
            self.member_name.to_string()
        } else {
            format!("${}", self.member_name.trim_start_matches('$'))
        };
        match owner {
            Some(owner) => format!("{owner}::{member}"),
            None => member,
        }
    }
}

/// Check whether a subject text is a chain rooted in `$this`, `self`,
/// `static`, or `parent` (`"$this->relation()"`,
/// `"static::where('x', 'y')"`, `"self::$prop"`).
fn subject_text_is_rooted_in_self(subject_text: &str) -> bool {
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
