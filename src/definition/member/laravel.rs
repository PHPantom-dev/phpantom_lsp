//! Laravel lookups for member go-to-definition: Eloquent virtual
//! attributes and their array entries, scopes, timestamp constants,
//! Builder and facade forwarding, and macro registration sites.

use std::sync::Arc;
use tower_lsp::lsp_types::*;

use super::{MemberKind, MemberLookup};
use crate::Backend;
use crate::definition::point_location;
use crate::types::*;
use crate::virtual_members::ResolvedClassCache;
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, custom_builder_fqn, extends_eloquent_model, facade_concrete_class,
};

impl Backend {
    /// Jump from a timestamp column property (`created_at`) on a model to
    /// the `CREATED_AT` / `UPDATED_AT` constant that names it.
    pub(super) fn timestamp_constant_location(
        &self,
        lookup: &MemberLookup<'_>,
        model: &ClassInfo,
        property_name: &str,
    ) -> Option<Location> {
        // Base resolution sees a `CREATED_AT` a parent model overrides.
        let resolved =
            crate::virtual_members::resolve_class_base_cached(model, lookup.class_loader);
        let const_name = Self::timestamp_property_to_constant(&resolved, property_name)?;
        let (const_class, const_fqn) =
            Self::find_declaring_class(model, const_name, lookup.class_loader)?;
        self.member_position_location(
            &const_fqn,
            lookup.uri,
            lookup.content,
            &const_class,
            const_name,
            MemberKind::Constant,
        )
    }

    /// Look up an Eloquent virtual array entry's position (a `$casts`,
    /// `$fillable`, `$dates`, etc. string literal), bundling the same
    /// `find_class_file_content` → `find_eloquent_array_entry` →
    /// `point_location` sequence shared by every array-entry fallback.
    pub(super) fn eloquent_array_entry_location(
        &self,
        lookup: &MemberLookup<'_>,
        declaring_fqn: &str,
        entry_name: &str,
        class: &ClassInfo,
    ) -> Option<Location> {
        let (class_uri, class_content) =
            self.find_class_file_content(declaring_fqn, lookup.uri, lookup.content)?;
        let class_bounds = (class.start_offset as usize, class.end_offset as usize);
        let entry_position =
            Self::find_eloquent_array_entry(&class_content, entry_name, Some(class_bounds))?;
        let parsed_uri = Url::parse(&class_uri).ok()?;
        Some(point_location(parsed_uri, entry_position))
    }

    /// Find where a static call on a Laravel facade is declared on the
    /// concrete class the facade forwards to (or on something that class
    /// inherits or mixes in).
    pub(super) fn find_facade_forwarded_method(
        class: &ClassInfo,
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        cache: Option<&ResolvedClassCache>,
    ) -> Option<(ClassInfo, String)> {
        let concrete = facade_concrete_class(class, class_loader, cache)?;
        let (declaring_class, fqn) =
            Self::find_declaring_class(&concrete, member_name, class_loader)?;

        // A member declared on the concrete class itself comes back under
        // its short name; use the FQN so the file lookup can disambiguate.
        if !fqn.contains('\\') && fqn == concrete.name {
            Some((declaring_class, concrete.fqn().to_string()))
        } else {
            Some((declaring_class, fqn))
        }
    }

    /// Locate the `::macro('name', ...)` registration site for a Laravel macro
    /// method named `name` on the class `fqn`.
    ///
    /// A macro is synthesized onto its target class but has no declaration in
    /// that class's own file, so the ordinary declaring-class file lookup finds
    /// nothing.  This jumps to the name literal of the registration call
    /// instead (typically in a service provider).  Returns `None` when the
    /// project has no macros or the member is not a recognized macro.
    pub(super) fn macro_definition_location(&self, fqn: &str, name: &str) -> Option<Location> {
        if !self
            .laravel_has_macros
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }
        let (uri, offset) = {
            let index = self.laravel_macros.read();
            let (uri, offset) = index.definition(fqn, name)?;
            (uri.to_string(), offset)
        };
        let content = self.get_file_content(&uri)?;
        let position = crate::text_position::offset_to_position(&content, offset as usize + 1);
        let parsed_uri = Url::parse(&uri).ok()?;
        Some(point_location(parsed_uri, position))
    }

    /// Map a virtual scope method name to the underlying `scopeXxx` method.
    ///
    /// Laravel scope methods are defined as `scopeActive(Builder $query)`
    /// but invoked as `active()` (or `BlogAuthor::active()`).  This helper
    /// converts `"active"` → `"scopeActive"` so that go-to-definition can
    /// find the actual method declaration.
    pub(super) fn scope_method_name(member_name: &str) -> String {
        let mut scope = String::with_capacity("scope".len() + member_name.len());
        scope.push_str("scope");
        let mut chars = member_name.chars();
        if let Some(first) = chars.next() {
            scope.extend(first.to_uppercase());
            scope.extend(chars);
        }
        scope
    }

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
    pub(super) fn find_builder_forwarded_method(
        class: &ClassInfo,
        member_name: &str,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<(ClassInfo, String)> {
        if !extends_eloquent_model(class, class_loader) {
            return None;
        }

        let builder_fqn = custom_builder_fqn(class, class_loader)
            .unwrap_or_else(|| ELOQUENT_BUILDER_FQN.to_string());

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
    /// methods injected by `type_hint_to_classes_typed`).  We use it to
    /// confirm the member exists and to extract the model name from the
    /// scope method's return type.
    ///
    /// Returns `Some((declaring_class, fqn, scope_method_name))` when
    /// the scope is found on the model, or `None` otherwise.
    pub(super) fn find_scope_on_builder_model(
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
            match ret.kind() {
                crate::php_type::TypeKind::Generic(g) if !g.args.is_empty() => {
                    // Check that the base type is the Eloquent Builder.
                    if g.name != ELOQUENT_BUILDER_FQN && g.name != "Builder" {
                        return None;
                    }
                    g.args[0].base_name().map(|s| s.to_string())
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
            .and_then(extract_model_from_builder_ret)
            .or_else(|| {
                resolved_candidate.methods.iter().find_map(|m| {
                    if m.is_static {
                        return None;
                    }
                    m.return_type
                        .as_ref()
                        .and_then(extract_model_from_builder_ret)
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
