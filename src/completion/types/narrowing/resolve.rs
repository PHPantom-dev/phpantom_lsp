//! Shared resolution helpers for type narrowing: subject-key
//! extraction and resolving class-name lists into `ClassInfo` unions.

use std::sync::Arc;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::types::ClassInfo;

use mago_syntax::cst::*;

use crate::completion::resolver::VarResolutionCtx;

use super::*;

/// Resolve the `class_type` inside an `InstanceofExtraction` to its FQN.
///
/// When the extractor returns a short class name (e.g. `Foo`), the
/// `class_loader` may know the fully-qualified name (`App\Foo`).
/// Resolving early ensures that downstream comparisons (e.g.
/// `out.contains(&cls_type)`) and `ResolvedType` hints carry the FQN
/// rather than the short name.
pub(in crate::completion) fn resolve_extraction_to_fqn(
    extraction: &mut InstanceofExtraction,
    class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<ClassInfo>>,
) {
    if let PhpType::Named(ref name) = extraction.class_type {
        let resolved = crate::util::resolve_name_via_loader(name, class_loader);
        if resolved != *name {
            extraction.class_type = PhpType::Named(resolved);
        }
    }
}

/// Resolve a list of `PhpType` values into a deduplicated `Vec<ClassInfo>`.
///
/// This is a shared helper for the compound instanceof/assert narrowing
/// patterns that produce a union of classes from multiple branches.
pub(crate) fn resolve_class_names_to_union(
    classes: &[PhpType],
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ClassInfo> {
    let mut union = Vec::new();
    for ty in classes {
        let resolved = super::super::resolution::type_hint_to_classes_typed(
            ty,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        for arc_cls in resolved {
            let cls = Arc::unwrap_or_clone(arc_cls);
            if !union.iter().any(|c: &ClassInfo| c.name == cls.name) {
                union.push(cls);
            }
        }
    }
    union
}

/// Convert an AST expression to a subject key string for narrowing comparison.
///
/// Handles:
/// - `$var` → `"$var"`
/// - `$this->prop` → `"$this->prop"`
/// - `$this?->prop` → `"$this->prop"` (null-safe normalised)
///
/// Returns `None` for expressions that are not supported as narrowing subjects.
pub(in crate::completion) fn expr_to_subject_key(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => Some(bytes_to_str(dv.name).to_string()),
        Expression::Access(Access::Property(pa)) => {
            let obj = expr_to_subject_key(pa.object)?;
            if let ClassLikeMemberSelector::Identifier(ident) = &pa.property {
                Some(format!("{}->{}", obj, bytes_to_str(ident.value)))
            } else {
                None
            }
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            let obj = expr_to_subject_key(pa.object)?;
            if let ClassLikeMemberSelector::Identifier(ident) = &pa.property {
                Some(format!("{}->{}", obj, bytes_to_str(ident.value)))
            } else {
                None
            }
        }
        Expression::ArrayAccess(aa) => {
            let base = expr_to_subject_key(aa.array)?;
            let key = array_access_key_as_string(aa)?;
            Some(format!("{}[\"{}\"]", base, key))
        }
        // See through parentheses so `($x instanceof Foo)` and grouped
        // subjects resolve to the same key as the bare form.
        Expression::Parenthesized(inner) => expr_to_subject_key(inner.expression),
        // Inline assignment as a subject: `($node = expr()) instanceof Foo`
        // narrows the assigned variable, so key on the assignment target.
        Expression::Assignment(assign) => expr_to_subject_key(assign.lhs),
        _ => None,
    }
}

/// Extract a literal key from an array access expression.
///
/// Returns the key string for `$a["test"]`, `$a['test']`, and `$a[0]`
/// (integer indices are stringified, matching PHP's integer/string key
/// coercion so `$a[0]` and `$a["0"]` narrow the same subject).  Returns
/// `None` for non-literal keys like `$a[$i]`.
pub(in crate::completion) fn array_access_key_as_string(
    aa: &mago_syntax::cst::ArrayAccess<'_>,
) -> Option<String> {
    use mago_syntax::cst::Literal;
    match aa.index {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the unquoted content; fall back to stripping
            // quotes from `raw`.
            let key = s
                .value
                .map(|v| bytes_to_str(v).to_string())
                .unwrap_or_else(|| {
                    let raw_str = bytes_to_str(s.raw);
                    crate::text_scan::unquote_php_string(raw_str)
                        .unwrap_or(raw_str)
                        .to_string()
                });
            Some(key)
        }
        Expression::Literal(Literal::Integer(i)) => {
            // PHP normalises integer-like keys, so `$a[0]` narrows the
            // same subject as `$a["0"]`.  Prefer the parsed value; fall
            // back to the raw token when it overflowed.
            i.value
                .map(|v| v.to_string())
                .or_else(|| Some(bytes_to_str(i.raw).to_string()))
        }
        _ => None,
    }
}
