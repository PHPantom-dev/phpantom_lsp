//! The name an Eloquent model method is used under.
//!
//! A scope, an accessor, or a mutator is declared under one name and used
//! under another: `scopeActive()` is called as `active()`,
//! `getFullNameAttribute()` and an `Attribute`-returning `fullName()` are
//! read as `->full_name`, `setLogoAttribute()` is written as `->logo = …`.
//! Find References and rename start from the declaration, so they need the
//! forward mapping the model provider applies when it synthesizes the
//! virtual members.

use crate::types::{MethodInfo, Visibility};

use super::accessors::{is_legacy_accessor, is_legacy_mutator, is_modern_accessor};
use super::helpers::snake_to_camel;
use super::helpers::{camel_to_snake, legacy_accessor_method_name, legacy_mutator_method_name};
use super::scopes::scope_name;

/// Which convention makes a model method reachable under a magic name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MagicMemberKind {
    /// `scopeActive()`, called as `active()`.
    Scope,
    /// A `#[Scope]` method, called under its own name, including on the
    /// model's query builder.
    AttributeScope,
    /// `getFullNameAttribute()`, read as `->full_name`.
    LegacyAccessor,
    /// `setFullNameAttribute()`, written as `->full_name = …`.
    LegacyMutator,
    /// `fullName(): Attribute`, used as `->full_name`.
    ModernAccessor,
}

impl MagicMemberKind {
    /// How a model uses `method`, or `None` when it is an ordinary method.
    ///
    /// The checks run in the order the model provider applies them, so a
    /// method that fits two conventions is classified the way it is
    /// synthesized.
    pub(crate) fn of(method: &MethodInfo) -> Option<Self> {
        if method.has_scope_attribute {
            // Laravel ignores a public `#[Scope]` method.
            return (method.visibility != Visibility::Public).then_some(Self::AttributeScope);
        }
        if method.name.starts_with("scope") && method.name.len() > 5 {
            return Some(Self::Scope);
        }
        if is_legacy_accessor(method) {
            return Some(Self::LegacyAccessor);
        }
        if is_legacy_mutator(method) {
            return Some(Self::LegacyMutator);
        }
        if is_modern_accessor(method) {
            return Some(Self::ModernAccessor);
        }
        None
    }

    /// Whether the magic name is called (`->active()`) rather than read or
    /// written as a property (`->full_name`).
    pub(crate) fn is_method(self) -> bool {
        matches!(self, Self::Scope | Self::AttributeScope)
    }

    /// The name a method called `method_name` is used under, or `None`
    /// when that name does not follow this kind's convention.
    ///
    /// Rename asks this of the new name, so it cannot assume the name was
    /// classified already.
    pub(crate) fn use_name(self, method_name: &str) -> Option<String> {
        match self {
            Self::Scope => (method_name.starts_with("scope") && method_name.len() > 5)
                .then(|| scope_name(method_name)),
            Self::AttributeScope => Some(method_name.to_string()),
            Self::LegacyAccessor => attribute_method_middle(method_name, "get").map(camel_to_snake),
            Self::LegacyMutator => attribute_method_middle(method_name, "set").map(camel_to_snake),
            Self::ModernAccessor => Some(camel_to_snake(method_name)),
        }
    }
}

/// The `FullName` of `getFullNameAttribute`, for the given prefix.
fn attribute_method_middle<'a>(method_name: &'a str, prefix: &str) -> Option<&'a str> {
    let middle = method_name
        .strip_prefix(prefix)?
        .strip_suffix("Attribute")?;
    middle
        .starts_with(|c: char| c.is_uppercase())
        .then_some(middle)
}

/// The method names that can declare a member used as `use_name`.
///
/// The inverse of [`MagicMemberKind::use_name`], for a caller that only
/// knows the name an access spells.  Names the access spells itself are
/// not repeated.
pub(crate) fn declaring_method_names(use_name: &str) -> impl Iterator<Item = String> {
    let mut scope = String::with_capacity("scope".len() + use_name.len());
    scope.push_str("scope");
    let mut chars = use_name.chars();
    if let Some(first) = chars.next() {
        scope.extend(first.to_uppercase());
        scope.push_str(chars.as_str());
    }
    let camel = snake_to_camel(use_name);
    [
        Some(scope),
        Some(legacy_accessor_method_name(use_name)),
        Some(legacy_mutator_method_name(use_name)),
        (camel != use_name).then_some(camel),
    ]
    .into_iter()
    .flatten()
}

#[cfg(test)]
#[path = "magic_uses_tests.rs"]
mod tests;
