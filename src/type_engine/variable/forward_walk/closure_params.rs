//! Seeding a closure's or arrow function's parameters.
//!
//! A closure's declaration is read the way a function's is
//! ([`declared_param_type`]), from the docblock attached to the closure
//! itself.  The callable signature of the call it is passed to then fills
//! in what the declaration leaves open.

use super::*;
use std::sync::Arc;

use crate::atom::bytes_to_str;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

/// The `/** … */` docblock directly attached to the code at
/// `fn_offset` — i.e. only whitespace, the `static`/`function` keywords,
/// or an assignment target separates its closing `*/` from `fn_offset`.
/// Restricting a closure's `@param` lookup to this block keeps
/// annotations from sibling closures/arrow functions from leaking across
/// statement boundaries, and keeps the lookup from scanning the file
/// above it.
fn adjacent_docblock(content: &str, fn_offset: usize) -> Option<&str> {
    let before = content.get(..fn_offset)?.trim_end();
    if let Some(doc) = docblock_ending_at(content, before.len()) {
        return Some(doc);
    }
    // Allow `static` keyword between docblock and `fn(…)`:
    //   /** @param T $x */ static fn(T $x) => …
    // Also allow the `function` keyword for regular closures.
    let before = before
        .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
        .trim_end();
    if let Some(doc) = docblock_ending_at(content, before.len()) {
        return Some(doc);
    }
    // A closure stored in a variable carries its docblock above the whole
    // statement, because that is where PHP attaches the comment:
    //
    //   /** @param Arg[] $callArgs */
    //   $setOffsetValueTypes = static function (array $callArgs) { … };
    //
    // Stepping back over the assignment target reaches it.  Only a plain
    // assignment is stepped over — a call argument or an array element is
    // not — which is what keeps a sibling closure's annotation out.
    let start = assignment_target_start(before)?;
    docblock_ending_at(content, before[..start].trim_end().len())
}

/// Byte offset of the assignment target in text that ends with a plain
/// `=`, or `None` when the text does not end in an assignment to a simple
/// lvalue (`$fn =`, `$this->fn =`, `$fns[] =`, `$fns['k'] =`).
fn assignment_target_start(before: &str) -> Option<usize> {
    let rest = before.strip_suffix('=')?;
    // Comparisons, arrows and compound assignments all put another
    // operator character right before the `=`; none of them assigns.
    if rest.ends_with(|c: char| "=!<>.+-*/%?&|^:".contains(c)) {
        return None;
    }
    let rest = rest.trim_end();
    let start = rest.rfind('$')?;
    rest[start..]
        .chars()
        .all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '$' | '[' | ']' | '-' | '>' | ':' | '\'' | '"')
        })
        .then_some(start)
}

/// Seed a closure/arrow function scope with parameter types, using
/// inferred callable types as fallback for untyped parameters.
///
/// Each parameter's declaration is read the way [`resolve_param_type`]
/// reads a function's.  `inferred_types` are the parameter types the
/// enclosing call's callable signature gives the closure, matched by
/// position; see [`refine_by_inferred`] for when one wins.
pub(crate) fn seed_closure_params(
    scope: &mut ScopeState,
    parameter_list: &FunctionLikeParameterList<'_>,
    fn_span_start: u32,
    inferred_types: &[PhpType],
    ctx: &ForwardWalkCtx<'_>,
) {
    let docblock = adjacent_docblock(ctx.file.content, fn_span_start as usize);
    let enclosing = EnclosingMethod {
        span_start: fn_span_start,
        name: None,
        has_scope_attr: false,
        trait_prototype: None,
    };
    for (idx, param) in parameter_list.parameters.iter().enumerate() {
        let pname = bytes_to_str(param.variable.name).to_string();

        // Only the docblock attached to this closure/arrow function is
        // read for its `@param` tags.
        let doc_type =
            docblock.and_then(|doc| crate::docblock::extract_param_raw_type(doc, &pname));
        let declared = declared_param_type(&pname, param, doc_type, &enclosing, ctx);
        let mut param_results = match inferred_types.get(idx) {
            Some(inferred) => refine_by_inferred(declared, inferred, ctx),
            None => declared.results,
        };

        if param.ellipsis.is_some() {
            wrap_variadic(&mut param_results);
        }

        // Closure/arrow-function parameters shadow same-named outer
        // variables unconditionally.  Even when no type could be
        // determined, the outer variable's type must not leak into the
        // closure body — record the parameter as present-but-untyped
        // and drop any synthetic keys (`$p->x`, `$p["k"]`) tracked for
        // the shadowed outer variable.
        scope.remove(&pname);
        scope.invalidate_dependent_keys(&pname);
        if param_results.is_empty() {
            scope.set_empty(&pname);
        } else {
            scope.seed(&pname, param_results);
        }
    }
}

/// A closure parameter's declared type, or `inferred`, the type the
/// callable signature it is passed to gives it, where that says more: when
/// the declaration names no type, when it names a class bare and the
/// signature that class with generic arguments, or when the signature's
/// classes refine the declared ones, or, for a declaration naming no class,
/// when the signature's type is a subtype of it.
fn refine_by_inferred(
    declared: DeclaredParam,
    inferred: &PhpType,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let Some(effective) = &declared.effective else {
        return inferred_param_results(inferred, ctx);
    };
    if super::super::closure_resolution::inferred_type_is_more_specific(
        effective,
        inferred,
        ctx.file.class_loader,
    ) {
        return inferred_param_results(inferred, ctx);
    }

    let declared_classes: Vec<&Arc<ClassInfo>> = declared
        .results
        .iter()
        .filter_map(|rt| rt.class_info.as_ref())
        .collect();
    if declared_classes.is_empty() {
        // The callable is only ever called with what the signature passes,
        // so a declared `string` holding `list<non-empty-string>`'s
        // elements is a `non-empty-string`.
        if !inferred.equivalent(effective)
            && crate::class_lookup::is_subtype_of_typed(inferred, effective, ctx.file.class_loader)
        {
            return inferred_param_results(inferred, ctx);
        }
        return declared.results;
    }
    let inferred_classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        inferred,
        &ctx.current_class.fqn(),
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    // Narrow to the inferred type only when it is a genuine refinement of
    // the *whole* declared type: every inferred class must be a subtype of
    // some declared class (inferred ⊆ declared), AND every declared class
    // must be refined by some inferred class (declared covered by
    // inferred).  Without the second check a declared union like `A|B|C`
    // collapses to a single inferred arm (`A`) when the subject is a union
    // of differently parameterized collections, discarding the other
    // declared possibilities.
    let refines = |inferred_cls: &Arc<ClassInfo>, declared_cls: &Arc<ClassInfo>| {
        crate::class_lookup::is_subtype_of_names(
            &inferred_cls.fqn(),
            &declared_cls.fqn(),
            ctx.file.class_loader,
        )
    };
    let inferred_is_subtype = !inferred_classes.is_empty()
        && inferred_classes.iter().all(|inferred_cls| {
            declared_classes
                .iter()
                .any(|declared_cls| refines(inferred_cls, declared_cls))
        });
    let inferred_covers_declared = declared_classes.iter().all(|declared_cls| {
        inferred_classes
            .iter()
            .any(|inferred_cls| refines(inferred_cls, declared_cls))
    });
    if inferred_is_subtype && inferred_covers_declared {
        ResolvedType::from_classes_with_hint(inferred_classes, inferred.clone())
    } else {
        declared.results
    }
}

/// What a parameter holds when the callable signature's type for it wins.
fn inferred_param_results(inferred: &PhpType, ctx: &ForwardWalkCtx<'_>) -> Vec<ResolvedType> {
    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        inferred,
        &ctx.current_class.fqn(),
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    if !resolved.is_empty() {
        ResolvedType::from_classes_with_hint(resolved, inferred.clone())
    } else if inferred.is_informative() {
        // A type that names no class (`array<int, array<string, string>>`
        // over a bare `array`) still says more than the declaration.
        vec![ResolvedType::from_type_string(inferred.clone())]
    } else {
        vec![]
    }
}
