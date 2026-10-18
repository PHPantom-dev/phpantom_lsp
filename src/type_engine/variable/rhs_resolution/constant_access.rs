//! Global constant reads: `PHP_EOL`, `SORT_ASC`, and the project's own
//! `const` and `define()` constants, typed from their initializers.

use mago_span::HasSpan;
use mago_syntax::cst::ConstantAccess;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::type_engine::resolver::VarResolutionCtx;
use crate::types::ResolvedType;
use crate::util::strip_fqn_prefix;

use super::infer_type_from_constant_value;

/// Resolve a global constant access to the type its value holds.
pub(super) fn resolve_rhs_constant_access(
    ca: &ConstantAccess<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let name = bytes_to_str(ca.name.value());
    let name_clean = strip_fqn_prefix(name);
    // `true`, `false`, `null` are parsed as ConstantAccess by
    // some AST variants — handle them the same as literals.
    match name_clean.to_lowercase().as_str() {
        "true" | "false" => {
            return vec![ResolvedType::from_type_string(PhpType::bool())];
        }
        "null" => {
            return vec![ResolvedType::from_type_string(PhpType::null())];
        }
        _ => {}
    }
    if let Some(ts) = global_constant_type(name, ca.name.span().start.offset, ctx) {
        return vec![ResolvedType::from_type_string(ts)];
    }
    crate::hover::constants::unversioned_php_version_constant_type(name_clean)
        .map(|ts| vec![ResolvedType::from_type_string(ts)])
        .unwrap_or_default()
}

/// The type the global constant `name` holds, read from its initializer.
///
/// `offset` is where `name` is written, or `0` when it was not written in
/// the file at all. A name the initializer itself uses is looked up through
/// the same constant loader, so an unqualified `ONE` in `const TWO = ONE * 2;`
/// finds the reading namespace's `ONE` the way the reference to `TWO` found
/// that namespace's `TWO`. The text-only resolver the fold falls back to has
/// no namespace to try.
fn global_constant_type(name: &str, offset: u32, ctx: &VarResolutionCtx<'_>) -> Option<PhpType> {
    let value = ctx.lookup_constant(name, offset)??;
    infer_type_from_constant_value(&value).or_else(|| {
        let rctx = ctx.as_resolution_ctx();
        let resolve = |text: &str| {
            let text = text.trim();
            let is_constant_name = text
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'\\')
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'\\');
            is_constant_name
                .then(|| global_constant_type(text, 0, ctx))
                .flatten()
                .or_else(|| crate::Backend::resolve_arg_text_to_type(text, &rctx))
        };
        crate::type_engine::types::const_fold::folded_constant_type(
            strip_fqn_prefix(name),
            &value,
            &resolve,
        )
    })
}
