//! Shared helpers for the Laravel virtual-member modules: case
//! conversion, parent-chain ancestry checks, accessor/mutator name
//! mapping, and reading string literals off the AST.

use std::sync::Arc;

use mago_syntax::cst::*;

use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH};

use super::ELOQUENT_MODEL_FQN;

/// Walk the parent chain of `class` checking whether any ancestor
/// (including the class itself) satisfies `predicate`.
///
/// This is the shared implementation behind the module tree's base-class
/// checks (`extends_eloquent_model`, `extends_eloquent_factory`, …).  The
/// predicate sees the class's own `name` first (which may be a short name;
/// callers that need it check `fqn()` separately), then each ancestor's
/// `parent_class` name, which post-processing has already resolved to an
/// FQN without a leading backslash.
pub(in crate::virtual_members::laravel) fn walks_parent_chain(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    predicate: fn(&str) -> bool,
) -> bool {
    if predicate(&class.name) {
        return true;
    }

    // Walk the parent chain without cloning ClassInfo.  We only need
    // each parent's `name` and `parent_class` fields, so keep a
    // cheap Arc handle instead of cloning the entire struct (which
    // copies hundreds of methods/properties/constants).
    let mut current_parent = class.parent_class;
    let mut depth = 0u32;
    while let Some(ref parent_name) = current_parent {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            break;
        }
        if predicate(parent_name) {
            return true;
        }
        match crate::class_lookup::load_resolved_name(parent_name, class_loader) {
            Some(parent) => {
                current_parent = parent.parent_class;
            }
            None => break,
        }
    }

    false
}

/// Determine whether `class_name` is the Eloquent Model base class.
pub(in crate::virtual_members::laravel) fn is_eloquent_model(class_name: &str) -> bool {
    class_name == ELOQUENT_MODEL_FQN
}

/// Walk the parent chain of `class` looking for
/// `Illuminate\Database\Eloquent\Model`.
///
/// Returns `true` if the class itself is `Model` or any ancestor is.
pub fn extends_eloquent_model(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    walks_parent_chain(class, class_loader, is_eloquent_model)
}

/// Determine whether `class_name` is the Eloquent Builder base class.
pub(in crate::virtual_members::laravel) fn is_eloquent_builder(class_name: &str) -> bool {
    class_name == super::ELOQUENT_BUILDER_FQN
}

/// Walk the parent chain of `class` looking for
/// `Illuminate\Database\Eloquent\Builder`.
pub fn extends_eloquent_builder(
    class: &ClassInfo,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    walks_parent_chain(class, class_loader, is_eloquent_builder)
}

/// Convert a camelCase or PascalCase string to snake_case.
///
/// Inserts an underscore before each uppercase letter that follows a
/// lowercase letter or digit, and before an uppercase letter that is
/// followed by a lowercase letter when preceded by another uppercase
/// letter, so acronyms stay whole (`URLName` → `url_name`).
///
/// Note this deliberately differs from Laravel's `Str::snake`, which
/// underscores every uppercase letter (`URLName` → `u_r_l_name`).  Both
/// spellings reach the same accessor at runtime because Eloquent maps
/// attribute names to methods via `Str::studly` + `method_exists`, and
/// PHP method lookup is case-insensitive — this form just produces the
/// friendlier property name.
///
/// `FullName` → `full_name`
/// `firstName` → `first_name`
/// `isAdmin` → `is_admin`
pub(crate) fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev = chars[i - 1];
                // Insert underscore when: lowercase/digit → uppercase,
                // or uppercase → uppercase followed by lowercase (acronym boundary).
                if prev.is_lowercase() || prev.is_ascii_digit() {
                    result.push('_');
                } else if prev.is_uppercase() {
                    // Acronym boundary: the last capital of a run starts
                    // a new word when followed by lowercase ("URLName" →
                    // "url_name").
                    if let Some(&next) = chars.get(i + 1)
                        && next.is_lowercase()
                    {
                        result.push('_');
                    }
                }
            }
            for lc in c.to_lowercase() {
                result.push(lc);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert a snake_case string to camelCase.
///
/// `full_name` → `fullName`
/// `avatar_url` → `avatarUrl`
/// `name` → `name`
pub(crate) fn snake_to_camel(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = false;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert a snake_case string to PascalCase.
///
/// `full_name` → `FullName`
/// `avatar_url` → `AvatarUrl`
/// `name` → `Name`
pub(in crate::virtual_members::laravel) fn snake_to_pascal(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Build the legacy accessor method name from a virtual property name.
///
/// `display_name` → `getDisplayNameAttribute`
/// `name` → `getNameAttribute`
pub(crate) fn legacy_accessor_method_name(property_name: &str) -> String {
    let pascal = snake_to_pascal(property_name);
    format!("get{pascal}Attribute")
}

pub(crate) fn legacy_mutator_method_name(property_name: &str) -> String {
    let pascal = snake_to_pascal(property_name);
    format!("set{pascal}Attribute")
}

/// Return candidate accessor method names for a virtual property name.
///
/// Go-to-definition uses this to map a snake_case virtual property back
/// to the method that produces it.  Returns the legacy accessor
/// (`getDisplayNameAttribute`), legacy mutator (`setDisplayNameAttribute`),
/// and modern (`displayName`) forms so the caller can try each one.
pub(crate) fn accessor_method_candidates(property_name: &str) -> Vec<String> {
    vec![
        legacy_accessor_method_name(property_name),
        legacy_mutator_method_name(property_name),
        snake_to_camel(property_name),
    ]
}

/// The first argument of a call, when it is a plain string literal.
pub(crate) fn first_string_arg<'c>(args: &ArgumentList<'_>, content: &'c str) -> Option<&'c str> {
    args.arguments
        .iter()
        .next()
        .and_then(|a| extract_string_literal(a.value(), content))
        .map(|(value, _, _)| value)
}

/// Extract the raw string value and inner byte offsets from a PHP string
/// literal expression.  Returns `(value, inner_start, inner_end)` where
/// `content[inner_start..inner_end]` is the string content without quotes.
pub(crate) fn extract_string_literal<'c>(
    expr: &Expression<'_>,
    content: &'c str,
) -> Option<(&'c str, usize, usize)> {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return None;
    };
    let start = s.span.start.offset as usize + 1;
    let end = s.span.end.offset as usize - 1;
    if start >= end || end > content.len() {
        return None;
    }
    Some((&content[start..end], start, end))
}

/// [`extract_string_literal`], but the text returned is the literal's
/// decoded runtime value (`'it\'s'` → `it's`) rather than the raw source
/// between the quotes. The span offsets still cover the source text.
pub(crate) fn resolved_string_literal<'arena>(
    expr: &Expression<'arena>,
    content: &str,
) -> Option<(&'arena str, usize, usize)> {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return None;
    };
    let start = s.span.start.offset as usize + 1;
    let end = s.span.end.offset as usize - 1;
    if start >= end || end > content.len() {
        return None;
    }
    let value = s.value.and_then(crate::atom::literal_bytes_to_str)?;
    Some((value, start, end))
}

/// [`extract_string_literal`] in the shape a span-emitting caller wants:
/// the literal's content and the offset it starts at, as a `u32`.
pub(crate) fn string_literal_at<'c>(
    expr: &Expression<'_>,
    content: &'c str,
) -> Option<(&'c str, u32)> {
    string_literal_at_range(expr, content).map(|(text, start, _)| (text, start))
}

/// [`string_literal_at`], with the offset the content ends at as well.
pub(crate) fn string_literal_at_range<'c>(
    expr: &Expression<'_>,
    content: &'c str,
) -> Option<(&'c str, u32, u32)> {
    extract_string_literal(expr, content).map(|(text, start, end)| (text, start as u32, end as u32))
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
