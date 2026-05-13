//! Member-access definition resolution.
//!
//! This module handles go-to-definition for member references — methods,
//! properties, and constants accessed via `->`, `?->`, or `::` operators.
//!
//! Supported patterns:
//!   - `$this->method()`, `$this->property`
//!   - `$var->method()`, `$var->property`
//!   - `self::method()`, `self::CONST`, `self::$staticProp`
//!   - `static::method()`, `parent::method()`
//!   - `ClassName::method()`, `ClassName::CONST`, `ClassName::$staticProp`
//!   - Chained access: `$this->prop->method()`, `app()->method()`
//!
//! Resolution walks the class hierarchy (parent classes, traits, mixins)
//! to find the declaring class and locates the member position in its
//! source file.
//!
//! Helper functions are split into submodules by responsibility:
//!   - [`declaring`] — inheritance chain walking (`find_declaring_class`,
//!     `find_declaring_in_traits`, `find_declaring_in_mixins`,
//!     `resolve_trait_alias`)
//!   - [`shape_keys`] — object shape property and Eloquent array entry
//!     position lookup
//!   - [`file_lookup`] — file loading (`find_class_file_content`,
//!     `reload_raw_class`) and member position lookup
//!     (`find_member_position`)

mod declaring;
mod file_lookup;
mod shape_keys;

use std::sync::Arc;
use tower_lsp::lsp_types::*;

use super::point_location;
use crate::Backend;
use crate::completion::resolver::ResolutionCtx;
use crate::docblock;
use crate::types::ResolvedType;
use crate::types::*;
use crate::util::{find_class_at_offset, position_to_offset};
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, accessor_method_candidates, count_property_to_relationship_method,
    extends_eloquent_model, is_accessor_method, where_property_method_to_column,
};

/// Pre-extracted context for a member definition lookup.
///
/// Bundles the four values that the caller assembles from the cursor
/// context so that [`Backend::resolve_member_definition_with`] does not
/// need seven separate parameters (plus `&self`).
pub(super) struct MemberDefinitionCtx<'a> {
    /// The member name under the cursor (e.g. `"method"`, `"prop"`).
    pub member_name: &'a str,
    /// The subject expression to the left of the access operator
    /// (e.g. `"$this"`, `"$var->prop"`, `"ClassName"`).
    pub subject: &'a str,
    /// Whether the access uses `::` or `->` / `?->`.
    pub access_kind: AccessKind,
    /// Hint about whether the site looks like a method call or property access.
    pub access_hint: MemberAccessHint,
}

/// The kind of class member being resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemberKind {
    Method,
    Property,
    Constant,
}

impl MemberKind {
    /// Return the string key used by [`ClassInfo::member_name_offset`].
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            MemberKind::Method => "method",
            MemberKind::Property => "property",
            MemberKind::Constant => "constant",
        }
    }
}

/// Hint about whether the member access looks like a method call or a property
/// access.  Used to disambiguate when a class has both a method and a property
/// with the same name (e.g. `id()` method vs `$id` property).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberAccessHint {
    /// Followed by `(` — looks like a method call.
    MethodCall,
    /// No `(` after the name — looks like a property / constant access.
    PropertyAccess,
    /// Cannot determine (fallback to original order).
    Unknown,
}

impl Backend {
    // ─── Member Definition Resolution ───────────────────────────────────────

    /// Resolve a member access to its definition using pre-extracted context.
    ///
    /// The caller provides a [`MemberDefinitionCtx`] bundling the subject
    /// text, access kind, and access hint so that the symbol-map path can
    /// drive resolution without re-extracting context from the source text.
    pub(super) fn resolve_member_definition_with(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        mctx: &MemberDefinitionCtx<'_>,
    ) -> Option<Location> {
        let member_name = mctx.member_name;
        let subject = mctx.subject;
        let access_kind = mctx.access_kind;
        let access_hint = mctx.access_hint;
        // 2. Gather context needed for class resolution.
        let cursor_offset = position_to_offset(content, position);
        let ctx = self.file_context(uri);

        let current_class = find_class_at_offset(&ctx.classes, cursor_offset).cloned();

        let class_loader = self.class_loader(&ctx);
        let function_loader = self.function_loader(&ctx);

        // 3. Resolve the subject to all candidate classes.
        //    When a variable is assigned different types in conditional
        //    branches (e.g. if/else), multiple candidates are returned.
        let rctx = ResolutionCtx {
            current_class: current_class.as_ref(),
            all_classes: &ctx.classes,
            content,
            cursor_offset,
            class_loader: &class_loader,
            resolved_class_cache: Some(&self.resolved_class_cache),
            function_loader: Some(&function_loader),
            scope_var_resolver: None,
            is_in_static_method: false,
        };
        let candidates = ResolvedType::into_arced_classes(
            crate::completion::resolver::resolve_target_classes(subject, access_kind, &rctx),
        );

        if candidates.is_empty() {
            return None;
        }

        // 4. Try each candidate class and pick the first one where the
        //    member actually exists (directly or via inheritance).
        for target_class in &candidates {
            // Candidates from resolve_target_classes may be fully-resolved
            // (merged) classes that include virtual/mixin members directly
            // in their methods list (e.g. when generic args triggered
            // resolve_class_fully inside type_hint_to_classes_typed).
            // find_declaring_class needs the raw (unmerged) class so it
            // can trace the member to the actual declaring class through
            // the real inheritance/mixin chain.
            let raw_class = Self::reload_raw_class(target_class, &ctx.classes, &class_loader);
            let lookup_class = raw_class.as_ref().unwrap_or(target_class);

            // Check if the member name is a trait `as` alias on this class.
            // If so, resolve to the original method name and (optionally) the
            // source trait so we jump to the actual method definition rather
            // than failing to find an alias that only exists after inheritance
            // resolution.
            let (effective_name, alias_trait) =
                Self::resolve_trait_alias(target_class, member_name);

            // When we matched an alias, jump directly to the trait method.
            // This is important when the class also declares a method with
            // the same name as the original (e.g. `use Foo { foo as __foo; }`
            // on a class that also has its own `foo()`).  Without this,
            // `find_declaring_class` would find the class's own `foo()`
            // instead of the trait's.
            let is_alias = effective_name != member_name;
            if is_alias {
                // Determine the source trait: use the explicit trait name
                // from a qualified alias (`TraitA::method as alias`), or
                // search the class's used traits for the original method.
                let source_trait_name = alias_trait.clone().or_else(|| {
                    Self::find_declaring_in_traits(
                        &target_class.used_traits,
                        &effective_name,
                        &class_loader,
                        0,
                    )
                    .map(|(_, fqn)| fqn)
                });

                if let Some(ref trait_name) = source_trait_name
                    && let Some(trait_info) = class_loader(trait_name)
                    && Self::classify_member(&trait_info, &effective_name, access_hint).is_some()
                    && let Some((class_uri, class_content)) =
                        self.find_class_file_content(trait_name, uri, content)
                    && let Some(member_position) = Self::find_member_position(
                        &class_content,
                        &effective_name,
                        MemberKind::Method,
                        trait_info.member_name_offset(&effective_name, "method"),
                    )
                    && let Ok(parsed_uri) = Url::parse(&class_uri)
                {
                    return Some(point_location(parsed_uri, member_position));
                }
            }

            // ── Timestamp constant redirect ─────────────────────────
            // When the property name matches a timestamp column,
            // jump straight to the CREATED_AT / UPDATED_AT constant.
            if extends_eloquent_model(lookup_class, &class_loader)
                && let Some(const_name) =
                    Self::timestamp_property_to_constant(lookup_class, &effective_name)
                && let Some((const_class, const_fqn)) =
                    Self::find_declaring_class(lookup_class, const_name, &class_loader)
                && let Some((class_uri, class_content)) =
                    self.find_class_file_content(&const_fqn, uri, content)
                && let Some(position) = Self::find_member_position(
                    &class_content,
                    const_name,
                    MemberKind::Constant,
                    const_class.member_name_offset(const_name, "constant"),
                )
                && let Ok(parsed_uri) = Url::parse(&class_uri)
            {
                return Some(point_location(parsed_uri, position));
            }

            // ── Scope method mapping ────────────────────────────────
            // Laravel scope methods are defined as `scopeActive()` but
            // invoked as `active()`.  When the effective name doesn't
            // exist as a real member, check if `scopeXxx` does and
            // redirect to that method definition instead.
            let scope_name = Self::scope_method_name(&effective_name);
            let (search_name, declaring_class, declaring_fqn) =
                match Self::find_declaring_class(lookup_class, &effective_name, &class_loader) {
                    Some((cls, fqn)) => (effective_name.clone(), cls, fqn),
                    None => {
                        // Try scope mapping: active → scopeActive
                        match Self::find_declaring_class(lookup_class, &scope_name, &class_loader) {
                            Some((cls, fqn)) => (scope_name.clone(), cls, fqn),
                            None => {
                                // Try scope-on-Builder: when the target
                                // is an Eloquent Builder<Model>, look
                                // for scopeXxx on the model class.
                                match Self::find_scope_on_builder_model(
                                    target_class,
                                    lookup_class,
                                    &effective_name,
                                    &class_loader,
                                ) {
                                    Some((cls, fqn, sname)) => (sname, cls, fqn),
                                    None => {
                                        // Try accessor mapping: display_name →
                                        // getDisplayNameAttribute or avatarUrl
                                        let accessor_match =
                                            accessor_method_candidates(&effective_name)
                                                .into_iter()
                                                .find_map(|candidate| {
                                                    Self::find_declaring_class(
                                                        lookup_class,
                                                        &candidate,
                                                        &class_loader,
                                                    )
                                                    .filter(|(cls, _)| {
                                                        is_accessor_method(cls, &candidate)
                                                    })
                                                    .map(|(cls, fqn)| (candidate, cls, fqn))
                                                });
                                        match accessor_match {
                                            Some((name, cls, fqn)) => (name, cls, fqn),
                                            None => {
                                                // Try *_count → relationship method mapping:
                                                // posts_count → posts, master_recipe_count → masterRecipe
                                                let count_match =
                                                    count_property_to_relationship_method(
                                                        target_class,
                                                        &effective_name,
                                                    )
                                                    .and_then(|rel_method| {
                                                        Self::find_declaring_class(
                                                            lookup_class,
                                                            &rel_method,
                                                            &class_loader,
                                                        )
                                                        .map(|(cls, fqn)| (rel_method, cls, fqn))
                                                    });
                                                match count_match {
                                                    Some((name, cls, fqn)) => (name, cls, fqn),
                                                    None => {
                                                        // Try builder-forwarded method: Laravel's
                                                        // Model::__callStatic delegates to Builder.
                                                        // The real Model has no @mixin, so we check
                                                        // explicitly.
                                                        match Self::find_builder_forwarded_method(
                                                            lookup_class,
                                                            &effective_name,
                                                            &class_loader,
                                                        ) {
                                                            Some((cls, fqn)) => {
                                                                (effective_name.clone(), cls, fqn)
                                                            }
                                                            None => (
                                                                effective_name.clone(),
                                                                ClassInfo::clone(target_class),
                                                                target_class.name.to_string(),
                                                            ),
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                };

            // ── Eloquent array entry fallback (pre-classify) ─────────────────
            // Virtual properties synthesised from $casts, $dates, $attributes,
            // $fillable, $guarded, $hidden, $visible, and $appends are NOT
            // stored in ClassInfo.properties — they exist only as virtual
            // members produced at resolve time.  classify_member therefore
            // returns None for them, causing us to `continue` past this
            // candidate before ever reaching the fallback below.  Check
            // the array entry here, before that early-exit, so that GTD on
            // $dates/$casts etc. still jumps to the correct string literal.
            if extends_eloquent_model(lookup_class, &class_loader)
                && let Some((class_uri, class_content)) =
                    self.find_class_file_content(&declaring_fqn, uri, content)
                && let Some(entry_position) = Self::find_eloquent_array_entry(
                    &class_content,
                    &effective_name,
                    Some((
                        declaring_class.start_offset as usize,
                        declaring_class.end_offset as usize,
                    )),
                )
                && let Ok(parsed_uri) = Url::parse(&class_uri)
            {
                return Some(point_location(parsed_uri, entry_position));
            }

            // ── where{Property} method → column entry fallback ───────────────
            // `whereFlour()`, `whereKitchenId()` etc. are virtual methods
            // synthesised from column names.  They have no real method
            // declaration; GTD should jump to the column's string literal
            // in the relevant Eloquent array ($fillable, $casts, etc.).
            if extends_eloquent_model(lookup_class, &class_loader)
                && let Some(column) = where_property_method_to_column(&effective_name)
                && let Some((class_uri, class_content)) =
                    self.find_class_file_content(&declaring_fqn, uri, content)
                && let Some(entry_position) = Self::find_eloquent_array_entry(
                    &class_content,
                    &column,
                    Some((
                        lookup_class.start_offset as usize,
                        lookup_class.end_offset as usize,
                    )),
                )
                && let Ok(parsed_uri) = Url::parse(&class_uri)
            {
                return Some(point_location(parsed_uri, entry_position));
            }

            // Check that the member is actually present on the declaring class.
            let member_kind =
                match Self::classify_member(&declaring_class, &search_name, access_hint) {
                    Some(k) => k,
                    None => continue, // member not on this candidate, try next
                };

            // Locate the file that contains the declaring class.
            if let Some((class_uri, class_content)) =
                self.find_class_file_content(&declaring_fqn, uri, content)
                && let Some(member_position) = Self::find_member_position(
                    &class_content,
                    &search_name,
                    member_kind,
                    declaring_class.member_name_offset(&search_name, member_kind.as_str()),
                )
                && let Ok(parsed_uri) = Url::parse(&class_uri)
            {
                return Some(point_location(parsed_uri, member_position));
            }

            // ── Object shape property fallback ──────────────────────
            // Synthetic `__object_shape` classes have no backing file.
            // Search the current file's docblocks for an `object{…}`
            // annotation that contains the property key and jump there.
            if declaring_fqn == "__object_shape"
                && let Some(position) = Self::find_object_shape_property_position(
                    content,
                    &search_name,
                    Some(cursor_offset as usize),
                )
                && let Ok(parsed_uri) = Url::parse(uri)
            {
                return Some(point_location(parsed_uri, position));
            }

            // ── Eloquent array entry fallback ───────────────────────
            // Virtual properties from $casts, $attributes, $fillable,
            // $guarded, $hidden, $visible, and $appends don't have a
            // method or property declaration.  Jump to the string literal
            // entry inside the array property instead.
            if extends_eloquent_model(lookup_class, &class_loader)
                && let Some((class_uri, class_content)) =
                    self.find_class_file_content(&declaring_fqn, uri, content)
                && let Some(entry_position) = Self::find_eloquent_array_entry(
                    &class_content,
                    &effective_name,
                    Some((
                        declaring_class.start_offset as usize,
                        declaring_class.end_offset as usize,
                    )),
                )
                && let Ok(parsed_uri) = Url::parse(&class_uri)
            {
                return Some(point_location(parsed_uri, entry_position));
            }
        }

        // No candidate had the member — fall back to the first candidate
        // and try the original (non-iterating) logic so we at least get
        // partial results when possible.
        let target_class = &candidates[0];
        let raw_fallback = Self::reload_raw_class(target_class, &ctx.classes, &class_loader);
        let fallback_class = raw_fallback.as_ref().unwrap_or(target_class);

        let (effective_name, alias_trait) = Self::resolve_trait_alias(target_class, member_name);

        // Direct trait lookup for aliased members in the fallback path.
        if let Some(ref trait_name) = alias_trait
            && let Some(ref trait_info) = class_loader(trait_name)
            && let Some((class_uri, class_content)) =
                self.find_class_file_content(trait_name, uri, content)
            && let Some(member_position) = Self::find_member_position(
                &class_content,
                &effective_name,
                MemberKind::Method,
                trait_info.member_name_offset(&effective_name, "method"),
            )
            && let Ok(parsed_uri) = Url::parse(&class_uri)
        {
            return Some(point_location(parsed_uri, member_position));
        }

        // Try with scope mapping in the fallback path too.
        let scope_name = Self::scope_method_name(&effective_name);
        let (search_name, declaring_class, declaring_fqn) = match Self::find_declaring_class(
            fallback_class,
            &effective_name,
            &class_loader,
        ) {
            Some((cls, fqn)) => (effective_name.clone(), cls, fqn),
            None => {
                match Self::find_declaring_class(fallback_class, &scope_name, &class_loader) {
                    Some((cls, fqn)) => (scope_name, cls, fqn),
                    None => {
                        // Try scope-on-Builder in the fallback path.
                        match Self::find_scope_on_builder_model(
                            target_class,
                            fallback_class,
                            &effective_name,
                            &class_loader,
                        ) {
                            Some((cls, fqn, sname)) => (sname, cls, fqn),
                            None => {
                                // Try accessor mapping in the fallback path.
                                let accessor_match = accessor_method_candidates(&effective_name)
                                    .into_iter()
                                    .find_map(|candidate| {
                                        Self::find_declaring_class(
                                            fallback_class,
                                            &candidate,
                                            &class_loader,
                                        )
                                        .filter(|(cls, _)| is_accessor_method(cls, &candidate))
                                        .map(|(cls, fqn)| (candidate, cls, fqn))
                                    });
                                match accessor_match {
                                    Some((name, cls, fqn)) => (name, cls, fqn),
                                    None => {
                                        // Try *_count → relationship method in fallback path.
                                        let count_match = count_property_to_relationship_method(
                                            target_class,
                                            &effective_name,
                                        )
                                        .and_then(|rel_method| {
                                            Self::find_declaring_class(
                                                fallback_class,
                                                &rel_method,
                                                &class_loader,
                                            )
                                            .map(|(cls, fqn)| (rel_method, cls, fqn))
                                        });
                                        match count_match {
                                            Some((name, cls, fqn)) => (name, cls, fqn),
                                            None => {
                                                match Self::find_builder_forwarded_method(
                                                    fallback_class,
                                                    &effective_name,
                                                    &class_loader,
                                                ) {
                                                    Some((cls, fqn)) => {
                                                        (effective_name.clone(), cls, fqn)
                                                    }
                                                    None => {
                                                        // Last resort: Eloquent array entry
                                                        // (virtual properties) and
                                                        // where{Property} method mapping.
                                                        if extends_eloquent_model(
                                                            fallback_class,
                                                            &class_loader,
                                                        ) {
                                                            let fqn =
                                                                fallback_class.name.to_string();
                                                            // where{Property} → column entry
                                                            if let Some(column) =
                                                                where_property_method_to_column(
                                                                    &effective_name,
                                                                )
                                                                && let Some((
                                                                    class_uri,
                                                                    class_content,
                                                                )) = self
                                                                    .find_class_file_content(
                                                                        &fqn, uri, content,
                                                                    )
                                                                && let Some(entry_position) =
                                                                    Self::find_eloquent_array_entry(
                                                                        &class_content,
                                                                        &column,
                                                                        Some((
                                                                            fallback_class
                                                                                .start_offset
                                                                                as usize,
                                                                            fallback_class
                                                                                .end_offset
                                                                                as usize,
                                                                        )),
                                                                    )
                                                                && let Ok(parsed_uri) =
                                                                    Url::parse(&class_uri)
                                                            {
                                                                return Some(point_location(
                                                                    parsed_uri,
                                                                    entry_position,
                                                                ));
                                                            }
                                                            if let Some((class_uri, class_content)) =
                                                                self.find_class_file_content(
                                                                    &fqn, uri, content,
                                                                )
                                                                && let Some(entry_position) =
                                                                    Self::find_eloquent_array_entry(
                                                                        &class_content,
                                                                        &effective_name,
                                                                        Some((
                                                                            fallback_class
                                                                                .start_offset
                                                                                as usize,
                                                                            fallback_class
                                                                                .end_offset
                                                                                as usize,
                                                                        )),
                                                                    )
                                                                && let Ok(parsed_uri) =
                                                                    Url::parse(&class_uri)
                                                            {
                                                                return Some(point_location(
                                                                    parsed_uri,
                                                                    entry_position,
                                                                ));
                                                            }
                                                        }
                                                        return None;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };

        let member_kind = Self::classify_member(&declaring_class, &search_name, access_hint)?;

        // ── Object shape property fallback (fallback path) ──────
        if declaring_fqn == "__object_shape"
            && let Some(position) = Self::find_object_shape_property_position(
                content,
                &search_name,
                Some(cursor_offset as usize),
            )
            && let Ok(parsed_uri) = Url::parse(uri)
        {
            return Some(point_location(parsed_uri, position));
        }

        let (class_uri, class_content) =
            self.find_class_file_content(&declaring_fqn, uri, content)?;

        let member_position = Self::find_member_position(
            &class_content,
            &search_name,
            member_kind,
            declaring_class.member_name_offset(&search_name, member_kind.as_str()),
        )?;

        let parsed_uri = Url::parse(&class_uri).ok()?;
        Some(point_location(parsed_uri, member_position))
    }

    // ─── Member Access Context Extraction ───────────────────────────────────

    /// Extract the subject and access kind for the member access under
    /// the cursor.
    ///
    /// Consults the precomputed symbol map (O(log n) lookup).
    ///
    /// Returns `(subject, AccessKind)` or `None` if the cursor is not on
    /// the RHS of a member access operator.
    pub(crate) fn lookup_member_access_context(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<(String, AccessKind)> {
        let offset = position_to_offset(content, position);

        // Try the symbol map (primary path).
        if let Some(result) = self.member_access_from_symbol_map(uri, offset) {
            return Some(result);
        }
        // Retry with offset − 1 for the end-of-token edge case (cursor
        // right after the last character of the member name).
        if offset > 0
            && let Some(result) = self.member_access_from_symbol_map(uri, offset - 1)
        {
            return Some(result);
        }

        None
    }

    /// Look up a `MemberAccess` symbol at `offset` in the symbol map and
    /// convert it to the `(subject, AccessKind)` pair expected by callers.
    fn member_access_from_symbol_map(
        &self,
        uri: &str,
        offset: u32,
    ) -> Option<(String, AccessKind)> {
        let maps = self.symbol_maps.read();
        let map = maps.get(uri)?;
        let span = map.lookup(offset)?;
        match &span.kind {
            crate::symbol_map::SymbolKind::MemberAccess {
                subject_text,
                is_static,
                ..
            } => {
                let access_kind = if *is_static {
                    AccessKind::DoubleColon
                } else {
                    AccessKind::Arrow
                };
                Some((subject_text.clone(), access_kind))
            }
            _ => None,
        }
    }

    // ─── Member Classification ──────────────────────────────────────────────

    /// Determine the kind of member (method, property, or constant) by
    /// checking the class's parsed information.
    ///
    /// Also checks `@method` and `@property` tags in the class's deferred
    /// docblock, since those are no longer parsed eagerly into
    /// `ClassInfo.methods` / `ClassInfo.properties`.
    ///
    /// Returns `None` if the member is not found in the class.
    fn classify_member(
        class: &ClassInfo,
        member_name: &str,
        hint: MemberAccessHint,
    ) -> Option<MemberKind> {
        let has_method = class.has_method(member_name);
        let has_property = class.properties.iter().any(|p| p.name == member_name);
        let has_constant = class.constants.iter().any(|c| c.name == member_name);

        // Also check the deferred class docblock for @method / @property
        // tags that are no longer in the parsed members.
        let (has_virtual_method, has_virtual_property) =
            Self::has_docblock_virtual_member(class, member_name);

        match hint {
            MemberAccessHint::PropertyAccess => {
                // Prefer property/constant over method when there's no `()`.
                if has_property || has_virtual_property {
                    return Some(MemberKind::Property);
                }
                if has_constant {
                    return Some(MemberKind::Constant);
                }
                if has_method || has_virtual_method {
                    return Some(MemberKind::Method);
                }
            }
            MemberAccessHint::MethodCall => {
                // Prefer method when followed by `()`.
                if has_method || has_virtual_method {
                    return Some(MemberKind::Method);
                }
                if has_property || has_virtual_property {
                    return Some(MemberKind::Property);
                }
                if has_constant {
                    return Some(MemberKind::Constant);
                }
            }
            MemberAccessHint::Unknown => {
                // Default order: method, property, constant.
                if has_method || has_virtual_method {
                    return Some(MemberKind::Method);
                }
                if has_property || has_virtual_property {
                    return Some(MemberKind::Property);
                }
                if has_constant {
                    return Some(MemberKind::Constant);
                }
            }
        }
        None
    }

    /// Check if a class's deferred docblock contains `@method` or `@property`
    /// tags that declare the given member name.
    ///
    /// Returns `(has_method, has_property)`.  This is a lazy parse of the
    /// class-level docblock that only runs when the member was not found
    /// among real declared members.
    fn has_docblock_virtual_member(class: &ClassInfo, member_name: &str) -> (bool, bool) {
        let doc_text = match class.class_docblock.as_deref() {
            Some(t) if !t.is_empty() => t,
            _ => return (false, false),
        };

        let has_method = docblock::extract_method_tags(doc_text)
            .iter()
            .any(|m| m.name == member_name);

        let has_property = docblock::extract_property_tags(doc_text)
            .iter()
            .any(|(name, _)| name == member_name);

        (has_method, has_property)
    }

    // ─── Scope Name Mapping ─────────────────────────────────────────────────

    /// Map a virtual scope method name to the underlying `scopeXxx` method.
    ///
    /// Laravel scope methods are defined as `scopeActive(Builder $query)`
    /// but invoked as `active()` (or `BlogAuthor::active()`).  This helper
    /// converts `"active"` → `"scopeActive"` so that go-to-definition can
    /// find the actual method declaration.
    fn scope_method_name(member_name: &str) -> String {
        let mut scope = String::with_capacity("scope".len() + member_name.len());
        scope.push_str("scope");
        let mut chars = member_name.chars();
        if let Some(first) = chars.next() {
            scope.extend(first.to_uppercase());
            scope.extend(chars);
        }
        scope
    }

    // ─── Eloquent Builder Forwarding ────────────────────────────────────────

    /// Check if a method is available on the Eloquent Builder for a Model
    /// subclass.
    ///
    /// Laravel's `Model::__callStatic()` forwards static calls to
    /// `Builder`, but the real `Model` class has no `@mixin Builder`
    /// annotation.  This function bridges that gap for go-to-definition
    /// by loading the Builder and searching its inheritance chain
    /// (including `@mixin Query\Builder` and traits like
    /// `BuildsQueries`) for the requested method.
    ///
    /// Returns `Some((ClassInfo, fqn))` of the declaring class when the
    /// method is found, or `None` if the class is not an Eloquent Model
    /// subclass or the method does not exist on Builder.
    fn find_builder_forwarded_method(
        class: &ClassInfo,
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<(ClassInfo, String)> {
        if !extends_eloquent_model(class, class_loader) {
            return None;
        }

        // Walk the parent chain to find a custom builder definition.
        // Laravel's #[UseEloquentBuilder] and HasBuilder are effectively inherited.
        let mut builder_fqn = ELOQUENT_BUILDER_FQN.to_string();
        let mut current = Some(class.clone());
        for _ in 0..MAX_INHERITANCE_DEPTH {
            let Some(curr) = current else { break };
            if let Some(name) = curr
                .laravel()
                .and_then(|l| l.custom_builder.as_ref())
                .and_then(|b| b.base_name())
            {
                builder_fqn = name.to_string();
                break;
            }
            current = curr
                .parent_class
                .as_ref()
                .and_then(|p| class_loader(p))
                .map(Arc::unwrap_or_clone);
        }

        let builder = class_loader(&builder_fqn)?;
        let (declaring_class, fqn) =
            Self::find_declaring_class(&builder, member_name, class_loader)?;

        // When the declaring class is the builder itself, find_declaring_class
        // returns the short name ("Builder"). Replace it with the FQN so
        // that find_class_file_content can disambiguate.
        if !fqn.contains('\\') && fqn == builder.name {
            Some((declaring_class, builder_fqn))
        } else {
            Some((declaring_class, fqn))
        }
    }

    /// Find a scope method's declaration on the model when the target
    /// class is an Eloquent Builder instance.
    ///
    /// When a variable resolves to `Builder<User>`, completion injects
    /// the model's scope methods onto the Builder.  For go-to-definition,
    /// we need to trace back to the `scopeXxx` method on the model.
    ///
    /// `resolved_candidate` is the fully-resolved Builder (with scope
    /// methods injected by `type_hint_to_classes_depth`).  We use it to
    /// confirm the member exists and to extract the model name from the
    /// scope method's return type.
    ///
    /// Returns `Some((declaring_class, fqn, scope_method_name))` when
    /// the scope is found on the model, or `None` otherwise.
    fn find_scope_on_builder_model(
        resolved_candidate: &ClassInfo,
        raw_class: &ClassInfo,
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<(ClassInfo, String, String)> {
        // Only applies to the Eloquent Builder class.
        let raw_fqn = raw_class.fqn();
        if raw_fqn != ELOQUENT_BUILDER_FQN {
            return None;
        }

        // Check if the resolved (scope-injected) candidate has this
        // method.  If not, the member is not a scope.
        let scope_method = resolved_candidate
            .methods
            .iter()
            .find(|m| m.name == member_name && !m.is_static)?;

        // Extract the model name from a Builder-typed return type.
        //
        // The return type is typically
        // `Illuminate\Database\Eloquent\Builder<App\Models\User>`.
        // We specifically look for return types whose base type is
        // the Eloquent Builder and extract the first generic arg as
        // the model name.
        let extract_model_from_builder_ret = |ret: &crate::php_type::PhpType| -> Option<String> {
            match ret {
                crate::php_type::PhpType::Generic(base, args) if !args.is_empty() => {
                    // Check that the base type is the Eloquent Builder.
                    if base != ELOQUENT_BUILDER_FQN && base != "Builder" {
                        return None;
                    }
                    args[0].base_name().map(|s| s.to_string())
                }
                _ => None,
            }
        };

        // When a scope declares a bare `Builder` return type (without
        // generic args like `<Model>`), the extraction above fails.
        // In that case, scan all other instance methods on the
        // resolved candidate for a Builder-typed return that carries
        // the model name.  All scope methods on the same
        // Builder<Model> instance share the same model, so any match
        // is valid.
        let model_name = scope_method
            .return_type
            .as_ref()
            .and_then(&extract_model_from_builder_ret)
            .or_else(|| {
                resolved_candidate.methods.iter().find_map(|m| {
                    if m.is_static {
                        return None;
                    }
                    m.return_type
                        .as_ref()
                        .and_then(&extract_model_from_builder_ret)
                })
            })?;

        // Load the model and verify it extends Eloquent Model.
        let model = class_loader(&model_name)?;
        if !extends_eloquent_model(&model, class_loader) {
            return None;
        }

        // Look for `scopeXxx` on the model's inheritance chain.
        // For `#[Scope]`-attributed methods, the declaration uses the
        // original name (e.g. `active`), not `scopeActive`.  Try the
        // `scopeX` convention first, then fall back to the original name.
        let scope_name = Self::scope_method_name(member_name);
        if let Some((declaring, fqn)) =
            Self::find_declaring_class(&model, &scope_name, class_loader)
        {
            return Some((declaring, fqn, scope_name));
        }

        // Fallback: `#[Scope]` attribute — the method keeps its own name.
        let (declaring, fqn) = Self::find_declaring_class(&model, member_name, class_loader)?;
        Some((declaring, fqn, member_name.to_string()))
    }

    /// Map a timestamp virtual property name to its defining constant.
    ///
    /// Returns `Some("CREATED_AT")` or `Some("UPDATED_AT")` when the
    /// property name matches the model's configured timestamp column,
    /// or `None` when the property is not a timestamp.
    fn timestamp_property_to_constant<'a>(
        class: &ClassInfo,
        property_name: &str,
    ) -> Option<&'a str> {
        if let Some(laravel) = class.laravel() {
            let created_col = match &laravel.created_at_name {
                Some(Some(name)) => Some(name.as_str()),
                Some(None) => None,
                None => Some("created_at"),
            };
            let updated_col = match &laravel.updated_at_name {
                Some(Some(name)) => Some(name.as_str()),
                Some(None) => None,
                None => Some("updated_at"),
            };
            if created_col == Some(property_name) {
                return Some("CREATED_AT");
            }
            if updated_col == Some(property_name) {
                return Some("UPDATED_AT");
            }
        }
        None
    }
}
