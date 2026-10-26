//! Incompatible trait property diagnostic.
//!
//! PHP lets a class pick up the same property from two traits (or from a
//! trait and its own body) only when both declarations agree on
//! visibility, `static`, `readonly`, native type, and default value.
//! Anything else is a compile-time fatal ("A and B define the same
//! property ($x) in the composition of C").  The inheritance merge keeps
//! the first declaration and drops the rest, so the conflict is checked
//! here against the unmerged declarations.

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::symbol_map::SymbolKind;
use crate::types::{ClassInfo, ClassLikeKind, MAX_TRAIT_DEPTH, PropertyInfo};

use super::helpers::{FileDiagnosticContext, make_diagnostic};

pub(crate) const INCOMPATIBLE_TRAIT_PROPERTY_CODE: &str = "incompatible_trait_property";

impl Backend {
    /// Collect incompatible-trait-property diagnostics for a single file.
    pub fn collect_trait_property_conflict_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_trait_property_conflict_diagnostics_with_context(&ctx, uri, content, out);
    }

    pub(crate) fn collect_trait_property_conflict_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let class_loaders = self.resolved_class_loaders(&ctx.file);

        for span in &ctx.symbol_map.spans {
            let SymbolKind::ClassDeclaration { name } = &span.kind else {
                continue;
            };
            let Some(class_info) = ctx.declared_class(name, span.start) else {
                continue;
            };
            if class_info.used_traits.is_empty()
                || !matches!(class_info.kind, ClassLikeKind::Class | ClassLikeKind::Trait)
            {
                continue;
            }
            let class_loader = class_loaders.at(span.start);

            let conflicts = find_conflicts(class_info, class_loader);
            if conflicts.is_empty() {
                continue;
            }
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) else {
                continue;
            };
            let composed = class_info.fqn();
            for conflict in conflicts {
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Error,
                    INCOMPATIBLE_TRAIT_PROPERTY_CODE,
                    format!(
                        "{} and {} define the same property (${}) in the composition of {}, but the definitions are incompatible",
                        conflict.first_owner, conflict.second_owner, conflict.property, composed
                    ),
                ));
            }
        }
    }
}

struct Conflict {
    property: Atom,
    first_owner: Atom,
    second_owner: Atom,
}

/// Compare every property a class declares itself or imports from its
/// directly used traits, in PHP's binding order (own body first, then
/// each trait in `use` order).  A trait's nested `use`s are folded into
/// that trait, matching how PHP has already composed the trait by the
/// time it is bound into the class; conflicts *inside* a trait are
/// reported on the trait's own declaration.
fn find_conflicts(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> Vec<Conflict> {
    let class_name = class.fqn();
    let mut seen: Vec<(Arc<PropertyInfo>, Atom)> = class
        .properties
        .iter()
        .filter(|p| !p.is_virtual)
        .map(|p| (Arc::clone(p), class_name))
        .collect();
    let mut conflicts = Vec::new();
    let mut reported: HashSet<Atom> = HashSet::new();

    for trait_name in &class.used_traits {
        let Some(trait_info) = class_loader(Fqn::new(trait_name)) else {
            continue;
        };
        let mut trait_props = Vec::new();
        collect_trait_properties(
            &trait_info,
            class_loader,
            &mut HashSet::new(),
            0,
            &mut trait_props,
        );
        for prop in trait_props {
            match seen.iter().find(|(p, _)| p.name == prop.name) {
                Some((existing, owner)) => {
                    if !compatible(existing, &prop) && reported.insert(prop.name) {
                        conflicts.push(Conflict {
                            property: prop.name,
                            first_owner: *owner,
                            second_owner: *trait_name,
                        });
                    }
                }
                None => seen.push((prop, *trait_name)),
            }
        }
    }
    conflicts
}

/// A trait's declared properties followed by those of the traits it uses,
/// first declaration of each name winning.
fn collect_trait_properties(
    trait_info: &ClassInfo,
    class_loader: &ResolvedClassLoader<'_>,
    visited: &mut HashSet<Atom>,
    depth: u32,
    out: &mut Vec<Arc<PropertyInfo>>,
) {
    if depth > MAX_TRAIT_DEPTH || !visited.insert(trait_info.fqn()) {
        return;
    }
    for prop in &trait_info.properties {
        if !prop.is_virtual && !out.iter().any(|p| p.name == prop.name) {
            out.push(Arc::clone(prop));
        }
    }
    for nested in &trait_info.used_traits {
        if let Some(nested_info) = class_loader(Fqn::new(nested)) {
            collect_trait_properties(&nested_info, class_loader, visited, depth + 1, out);
        }
    }
}

fn compatible(a: &PropertyInfo, b: &PropertyInfo) -> bool {
    if a.visibility != b.visibility || a.is_static != b.is_static || a.is_readonly != b.is_readonly
    {
        return false;
    }
    let types_match = match (&a.native_type_hint, &b.native_type_hint) {
        (None, None) => true,
        (Some(x), Some(y)) => x.equivalent(y),
        _ => false,
    };
    types_match && defaults_compatible(a, b)
}

/// The default value of a property, as far as it can be known statically.
#[derive(PartialEq)]
enum DefaultValue {
    /// A typed property without a default starts uninitialized.
    Uninitialized,
    Literal(Literal),
    /// A constant expression that cannot be compared without evaluating it.
    Opaque,
}

#[derive(PartialEq)]
enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Str(String),
}

fn default_of(prop: &PropertyInfo) -> DefaultValue {
    let text = match &prop.source {
        Some(crate::types::PropertySource::DeclaredDefault { value }) => value.trim(),
        _ if prop.native_type_hint.is_some() => return DefaultValue::Uninitialized,
        // An untyped property without a default is implicitly `null`.
        _ => return DefaultValue::Literal(Literal::Null),
    };
    match parse_literal(text) {
        Some(lit) => DefaultValue::Literal(lit),
        None => DefaultValue::Opaque,
    }
}

fn defaults_compatible(a: &PropertyInfo, b: &PropertyInfo) -> bool {
    match (default_of(a), default_of(b)) {
        (DefaultValue::Uninitialized, DefaultValue::Uninitialized) => true,
        (DefaultValue::Uninitialized, _) | (_, DefaultValue::Uninitialized) => false,
        (DefaultValue::Literal(x), DefaultValue::Literal(y)) => x == y,
        // PHP compares the evaluated values, which needs constant
        // evaluation; give the benefit of the doubt.
        _ => true,
    }
}

/// Parse a scalar literal into a value comparable with PHP's `===`.
/// Returns `None` for anything that would need evaluation (constants,
/// arrays, escaped or interpolated strings, arithmetic).
fn parse_literal(text: &str) -> Option<Literal> {
    if text.eq_ignore_ascii_case("null") {
        return Some(Literal::Null);
    }
    if text.eq_ignore_ascii_case("true") {
        return Some(Literal::Bool(true));
    }
    if text.eq_ignore_ascii_case("false") {
        return Some(Literal::Bool(false));
    }
    if text.len() >= 2 {
        let inner = &text[1..text.len() - 1];
        if text.starts_with('\'') && text.ends_with('\'') && !inner.contains(['\\', '\'']) {
            return Some(Literal::Str(inner.to_string()));
        }
        if text.starts_with('"') && text.ends_with('"') && !inner.contains(['\\', '"', '$']) {
            return Some(Literal::Str(inner.to_string()));
        }
    }

    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text),
    };
    let digits = digits.replace('_', "");
    let lower = digits.to_ascii_lowercase();
    let int = if let Some(hex) = lower.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = lower.strip_prefix("0b") {
        i64::from_str_radix(bin, 2).ok()
    } else if let Some(oct) = lower.strip_prefix("0o") {
        i64::from_str_radix(oct, 8).ok()
    } else if lower.len() > 1 && lower.starts_with('0') && lower.bytes().all(|b| b.is_ascii_digit())
    {
        i64::from_str_radix(&lower[1..], 8).ok()
    } else if !lower.is_empty() && lower.bytes().all(|b| b.is_ascii_digit()) {
        lower.parse::<i64>().ok()
    } else {
        None
    };
    if let Some(n) = int {
        return Some(Literal::Int(if negative { -n } else { n }));
    }
    if !lower.is_empty()
        && lower
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'+' | b'-'))
        && let Ok(f) = lower.parse::<f64>()
    {
        let f = if negative { -f } else { f };
        // `-0.0 === 0.0` in PHP.
        let f = if f == 0.0 { 0.0 } else { f };
        return Some(Literal::Float(f.to_bits()));
    }
    None
}
