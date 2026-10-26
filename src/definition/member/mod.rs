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
//!   - [`laravel`] — Eloquent attribute entries, scopes, timestamp
//!     constants, Builder and facade forwarding, and macro sites

mod declaring;
mod file_lookup;
mod laravel;
mod shape_keys;

use tower_lsp_server::gen_lsp_types::*;

use super::point_location;
use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::text_position::position_to_offset;
use crate::types::ResolvedType;
use crate::types::*;
use crate::virtual_members::ResolvedClassCache;
use crate::virtual_members::laravel::{
    accessor_method_candidates, count_property_to_relationship_method, extends_eloquent_model,
    is_accessor_or_mutator_method, where_property_method_to_column,
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

/// The request-wide inputs every phase of a member definition lookup
/// shares.
struct MemberLookup<'a> {
    uri: &'a str,
    content: &'a str,
    cursor_offset: u32,
    member_name: &'a str,
    access_hint: MemberAccessHint,
    class_loader: &'a ResolvedClassLoader<'a>,
}

impl Backend {
    // ─── Member Definition Resolution ───────────────────────────────────────

    /// Resolve a member named by external project metadata, where the class
    /// is already a fully-qualified name and there is no PHP source context
    /// to extract a subject expression from.  `is_property` prefers a
    /// property over a method of the same name, as a validation mapping or
    /// a form field means one.
    pub(crate) fn class_member_declaration_location(
        &self,
        class_fqn: &str,
        member_name: &str,
        is_property: bool,
    ) -> Option<Location> {
        let target_class = self.find_or_load_class(class_fqn)?;
        let class_loader = |name: Fqn<'_>| self.find_or_load_class(&name);
        let (declaring_class, declaring_fqn) =
            Self::find_declaring_class(&target_class, member_name, &class_loader)?;
        let hint = if is_property {
            MemberAccessHint::PropertyAccess
        } else {
            MemberAccessHint::Unknown
        };
        let member_kind = Self::classify_member(&declaring_class, member_name, hint)?;
        self.member_position_location(
            &declaring_fqn,
            "",
            "",
            &declaring_class,
            member_name,
            member_kind,
        )
    }

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
        let cursor_offset = position_to_offset(content, position);
        let ctx = self.file_context_at(uri, cursor_offset);
        let loaders = self.file_loaders(&ctx);
        let class_loader = loaders.resolved_class_loader();

        // When a variable is assigned different types in conditional
        // branches (e.g. if/else), the subject has several candidates.
        let candidates =
            ResolvedType::into_arced_classes(crate::type_engine::resolver::resolve_target_classes(
                mctx.subject,
                mctx.access_kind,
                &loaders.resolution_ctx_at(content, cursor_offset),
            ));
        let first = candidates.first()?;

        let lookup = MemberLookup {
            uri,
            content,
            cursor_offset,
            member_name: mctx.member_name,
            access_hint: mctx.access_hint,
            class_loader,
        };

        // Pick the first candidate where the member actually exists
        // (directly or via inheritance).
        for target_class in &candidates {
            // Candidates from resolve_target_classes may be fully-resolved
            // (merged) classes that include virtual/mixin members directly
            // in their methods list (e.g. when generic args triggered
            // resolve_class_fully inside type_hint_to_classes_typed).
            // find_declaring_class needs the raw (unmerged) class so it
            // can trace the member to the actual declaring class through
            // the real inheritance/mixin chain.
            let raw_class = Self::reload_raw_class(target_class, &ctx.classes, class_loader);
            let lookup_class = raw_class.as_ref().unwrap_or(target_class);
            if let Some(location) =
                self.member_definition_on_candidate(&lookup, target_class, lookup_class)
            {
                return Some(location);
            }
        }

        // No candidate had the member: fall back to the first candidate
        // so we at least get partial results when possible.
        let raw_fallback = Self::reload_raw_class(first, &ctx.classes, class_loader);
        self.member_definition_fallback(&lookup, first, raw_fallback.as_ref().unwrap_or(first))
    }

    /// Look the member up on one candidate class of the subject.
    ///
    /// `target_class` is the candidate as resolved (possibly merged with
    /// its virtual members); `lookup_class` is the same class as declared.
    fn member_definition_on_candidate(
        &self,
        lookup: &MemberLookup<'_>,
        target_class: &ClassInfo,
        lookup_class: &ClassInfo,
    ) -> Option<Location> {
        let (uri, content) = (lookup.uri, lookup.content);

        // A trait `as` alias resolves to the original method name and
        // (optionally) the source trait.  Jump straight to the trait method:
        // the class may also declare a method with the original name
        // (`use Foo { foo as __foo; }` beside its own `foo()`), which
        // `find_declaring_class` would find instead.
        let (effective_name, alias_trait) =
            Self::resolve_trait_alias(target_class, lookup.member_name);
        if effective_name != lookup.member_name
            && let Some(location) =
                self.trait_alias_location(lookup, target_class, &effective_name, alias_trait)
        {
            return Some(location);
        }

        let is_model = extends_eloquent_model(lookup_class, lookup.class_loader);
        if is_model
            && let Some(location) =
                self.timestamp_constant_location(lookup, lookup_class, &effective_name)
        {
            return Some(location);
        }

        let (search_name, declaring_class, declaring_fqn) = Self::resolve_search_name(
            target_class,
            lookup_class,
            &effective_name,
            lookup.class_loader,
            Some(&self.resolved_class_cache),
        )
        .unwrap_or_else(|| {
            (
                effective_name.clone(),
                ClassInfo::clone(target_class),
                target_class.fqn().to_string(),
            )
        });

        if lookup.access_hint == MemberAccessHint::PropertyAccess
            && is_accessor_or_mutator_method(&declaring_class, &search_name)
            && let Some(location) = self.member_position_location(
                &declaring_fqn,
                uri,
                content,
                &declaring_class,
                &search_name,
                MemberKind::Method,
            )
        {
            return Some(location);
        }

        let member_kind = Self::classify_member(&declaring_class, &search_name, lookup.access_hint);

        if is_model {
            // Virtual attributes synthesised from $casts, $dates,
            // $attributes, $fillable, $guarded, $hidden, $visible and
            // $appends have no declaration; jump to the string literal in
            // the array instead.
            if let Some(location) = self.eloquent_array_entry_location(
                lookup,
                &declaring_fqn,
                &effective_name,
                &declaring_class,
            ) {
                return Some(location);
            }
            // `whereFlour()`, `whereKitchenId()` etc. are synthesised from
            // column names; jump to the column's entry.  A real method of
            // that name, such as a `scopeWhereEmail` scope, is what Laravel
            // calls instead, so it wins.
            if member_kind.is_none()
                && let Some(column) = where_property_method_to_column(&effective_name)
                && let Some(location) = self.eloquent_array_entry_location(
                    lookup,
                    &declaring_fqn,
                    &column,
                    lookup_class,
                )
            {
                return Some(location);
            }
        }

        self.declared_member_location(
            lookup,
            &declaring_fqn,
            &declaring_class,
            &search_name,
            member_kind?,
        )
    }

    /// The fallback when no candidate declares the member: retry on the
    /// first candidate without requiring the member to classify up front.
    fn member_definition_fallback(
        &self,
        lookup: &MemberLookup<'_>,
        target_class: &ClassInfo,
        fallback_class: &ClassInfo,
    ) -> Option<Location> {
        let (effective_name, alias_trait) =
            Self::resolve_trait_alias(target_class, lookup.member_name);

        if let Some(ref trait_name) = alias_trait
            && let Some(ref trait_info) = (lookup.class_loader)(Fqn::new(trait_name))
            && let Some(location) = self.member_position_location(
                trait_name,
                lookup.uri,
                lookup.content,
                trait_info,
                &effective_name,
                MemberKind::Method,
            )
        {
            return Some(location);
        }

        let Some((search_name, declaring_class, declaring_fqn)) = Self::resolve_search_name(
            target_class,
            fallback_class,
            &effective_name,
            lookup.class_loader,
            Some(&self.resolved_class_cache),
        ) else {
            // Last resort: an Eloquent attribute's array entry, for the
            // attribute itself or for a where{Property} method.
            if !extends_eloquent_model(fallback_class, lookup.class_loader) {
                return None;
            }
            let fqn = fallback_class.fqn();
            return where_property_method_to_column(&effective_name)
                .and_then(|column| {
                    self.eloquent_array_entry_location(lookup, &fqn, &column, fallback_class)
                })
                .or_else(|| {
                    self.eloquent_array_entry_location(
                        lookup,
                        &fqn,
                        &effective_name,
                        fallback_class,
                    )
                });
        };

        let member_kind =
            Self::classify_member(&declaring_class, &search_name, lookup.access_hint)?;
        self.declared_member_location(
            lookup,
            &declaring_fqn,
            &declaring_class,
            &search_name,
            member_kind,
        )
    }

    /// Locate a member once its declaring class and kind are known: a
    /// macro's registration site, the declaration in the class's file, or
    /// a synthetic object shape's docblock key.
    fn declared_member_location(
        &self,
        lookup: &MemberLookup<'_>,
        declaring_fqn: &str,
        declaring_class: &ClassInfo,
        search_name: &str,
        member_kind: MemberKind,
    ) -> Option<Location> {
        // A macro method has no declaration in the declaring class's own
        // file; jump to its `::macro('name', ...)` registration instead.
        if member_kind == MemberKind::Method
            && let Some(location) =
                self.macro_definition_location(declaring_class.fqn().as_str(), search_name)
        {
            return Some(location);
        }

        if let Some(location) = self.member_position_location(
            declaring_fqn,
            lookup.uri,
            lookup.content,
            declaring_class,
            search_name,
            member_kind,
        ) {
            return Some(location);
        }

        // Synthetic `__object_shape` classes have no backing file.  Search
        // the current file's docblocks for an `object{…}` annotation that
        // contains the property key.
        if declaring_fqn == "__object_shape" {
            return self.object_shape_property_location(
                lookup.uri,
                lookup.content,
                search_name,
                lookup.cursor_offset,
            );
        }
        None
    }

    /// Jump to the trait method an `as` alias names, through the trait the
    /// alias qualifies (`TraitA::method as alias`) or else the used trait
    /// that declares the original method.
    fn trait_alias_location(
        &self,
        lookup: &MemberLookup<'_>,
        target_class: &ClassInfo,
        effective_name: &str,
        alias_trait: Option<String>,
    ) -> Option<Location> {
        let trait_name = alias_trait.or_else(|| {
            Self::find_declaring_in_traits(
                &target_class.used_traits,
                effective_name,
                lookup.class_loader,
            )
            .map(|(_, fqn)| fqn)
        })?;
        let trait_info = (lookup.class_loader)(Fqn::new(&trait_name))?;
        Self::classify_member(&trait_info, effective_name, lookup.access_hint)?;
        self.member_position_location(
            &trait_name,
            lookup.uri,
            lookup.content,
            &trait_info,
            effective_name,
            MemberKind::Method,
        )
    }

    /// Look up a member's declaration position given its declaring class
    /// and FQN, bundling the `find_class_file_content` → `find_member_position`
    /// → `point_location` sequence shared by every member-position lookup
    /// in this module.
    fn member_position_location(
        &self,
        declaring_fqn: &str,
        uri: &str,
        content: &str,
        declaring_class: &ClassInfo,
        search_name: &str,
        member_kind: MemberKind,
    ) -> Option<Location> {
        let (class_uri, class_content) =
            self.find_class_file_content(declaring_fqn, uri, content)?;
        if crate::stubs::is_embedded_stub_uri(&class_uri) {
            return None;
        }
        let member_position =
            Self::find_member_position(&class_content, search_name, member_kind, declaring_class)?;
        let parsed_uri = crate::util::parse_url(&class_uri)?;
        Some(point_location(parsed_uri, member_position))
    }

    /// Look up a synthetic `__object_shape` property's position in the
    /// current file's docblocks.
    fn object_shape_property_location(
        &self,
        uri: &str,
        content: &str,
        search_name: &str,
        cursor_offset: u32,
    ) -> Option<Location> {
        let position = Self::find_object_shape_property_position(
            content,
            search_name,
            Some(cursor_offset as usize),
        )?;
        let parsed_uri = crate::util::parse_url(uri)?;
        Some(point_location(parsed_uri, position))
    }

    /// Resolve `effective_name` on `lookup_class` through the shared
    /// virtual-member fallback cascade: the member itself, the `scopeXxx`
    /// convention, a scope method injected onto an Eloquent Builder, an
    /// accessor/mutator method, a `*_count` relationship method, and a
    /// Laravel Builder-forwarded method. `target_class` is the original
    /// (possibly merged) candidate class, used by the Builder/relationship
    /// checks that need generic args or virtual methods not present on the
    /// raw `lookup_class`.
    ///
    /// Returns `None` when none of these resolve, leaving the caller to
    /// decide its own fallback.
    fn resolve_search_name(
        target_class: &ClassInfo,
        lookup_class: &ClassInfo,
        effective_name: &str,
        class_loader: &ResolvedClassLoader<'_>,
        cache: Option<&ResolvedClassCache>,
    ) -> Option<(String, ClassInfo, String)> {
        if let Some((cls, fqn)) =
            Self::find_declaring_class(lookup_class, effective_name, class_loader)
        {
            return Some((effective_name.to_string(), cls, fqn));
        }

        // Try scope mapping: active → scopeActive
        let scope_name = Self::scope_method_name(effective_name);
        if let Some((cls, fqn)) =
            Self::find_declaring_class(lookup_class, &scope_name, class_loader)
        {
            return Some((scope_name, cls, fqn));
        }

        // Try scope-on-Builder: when the target is an Eloquent
        // Builder<Model>, look for scopeXxx on the model class.
        if let Some((cls, fqn, sname)) = Self::find_scope_on_builder_model(
            target_class,
            lookup_class,
            effective_name,
            class_loader,
        ) {
            return Some((sname, cls, fqn));
        }

        // Try accessor mapping: display_name → getDisplayNameAttribute or avatarUrl
        if let Some((name, cls, fqn)) = accessor_method_candidates(effective_name)
            .into_iter()
            .find_map(|candidate| {
                Self::find_declaring_class(lookup_class, &candidate, class_loader)
                    .filter(|(cls, _)| is_accessor_or_mutator_method(cls, &candidate))
                    .map(|(cls, fqn)| (candidate, cls, fqn))
            })
        {
            return Some((name, cls, fqn));
        }

        // Try *_count → relationship method mapping:
        // posts_count → posts, master_recipe_count → masterRecipe
        if let Some((name, cls, fqn)) =
            count_property_to_relationship_method(target_class, effective_name).and_then(
                |rel_method| {
                    Self::find_declaring_class(lookup_class, &rel_method, class_loader)
                        .map(|(cls, fqn)| (rel_method, cls, fqn))
                },
            )
        {
            return Some((name, cls, fqn));
        }

        // Try builder-forwarded method: Laravel's Model::__callStatic
        // delegates to Builder. The real Model has no @mixin, so we check
        // explicitly.
        if let Some((cls, fqn)) =
            Self::find_builder_forwarded_method(lookup_class, effective_name, class_loader)
        {
            return Some((effective_name.to_string(), cls, fqn));
        }

        // Try facade-forwarded method: `Facade::__callStatic` hands the
        // call to the instance its `getFacadeAccessor()` names.
        if let Some((cls, fqn)) =
            Self::find_facade_forwarded_method(lookup_class, effective_name, class_loader, cache)
        {
            return Some((effective_name.to_string(), cls, fqn));
        }

        None
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

        if let Some(result) = self.member_access_from_symbol_map(uri, content, offset) {
            return Some(result);
        }
        // Retry with offset − 1 for the end-of-token edge case (cursor
        // right after the last character of the member name).
        if offset > 0
            && let Some(result) = self.member_access_from_symbol_map(uri, content, offset - 1)
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
        content: &str,
        offset: u32,
    ) -> Option<(String, AccessKind)> {
        let map = self.symbol_map_for(uri)?;
        let source = map.source(content)?;
        let span = map.lookup(offset)?;
        match &span.kind {
            crate::symbol_map::SymbolKind::MemberAccess {
                subject_text,
                is_static,
                ..
            } => {
                let access_kind = AccessKind::of_member_access(*is_static);
                Some((subject_text.as_str(source).to_string(), access_kind))
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

    /// Check if a class's docblock declares `@method` or `@property` tags
    /// for the given member name.
    ///
    /// Returns `(has_method, has_property)`.
    fn has_docblock_virtual_member(class: &ClassInfo, member_name: &str) -> (bool, bool) {
        let has_method = class.doc_methods().iter().any(|m| m.name == member_name);
        let has_property = class
            .doc_properties()
            .iter()
            .any(|(name, _)| name == member_name);

        (has_method, has_property)
    }
}
