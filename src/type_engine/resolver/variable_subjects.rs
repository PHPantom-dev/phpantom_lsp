//! Variable subjects: `$this`, a bare `$var`, and a member named by a
//! `$variable` (`$obj->$name()`), each read through the forward walker's
//! scope so narrowing applies to them.

use crate::php_type::TypeKind;
use crate::type_engine::variable::resolution::VarQuery;
use crate::types::*;

use super::{ResolutionCtx, is_bare_variable};

/// Resolve `$this`: the enclosing class, unless a closure binds it to
/// something else or the forward walker narrowed it.
pub(super) fn resolve_this_subject(ctx: &ResolutionCtx<'_>) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.file.all_classes;
    let class_loader = ctx.file.class_loader;

    // `$this` is not available inside static methods.
    if current_class.is_some() && ctx.is_in_static_method {
        return vec![];
    }

    // Consult the forward-walk scope for a narrowed or seeded
    // `$this` type.  This covers two cases the lexical
    // `current_class` fallback cannot:
    //   - `assert($this instanceof X)` inside a top-level closure
    //     (e.g. a Pest test) where there is no enclosing class,
    //     so `current_class` is `None`.
    //   - `instanceof` narrowing of `$this` to a subclass inside
    //     a regular method body.
    // When the scope yields nothing, fall back to the lexical
    // `current_class` below.
    let from_scope = resolve_this_from_scope(ctx);

    // `@param-closure-this` override: when the cursor is inside a
    // closure passed as an argument to a function whose parameter
    // carries the tag, `$this` is the declared type rather than the
    // lexical class.  The tag states what the closure is *bound* to,
    // so a narrowing proof inside the body still refines it — a
    // `assert($this instanceof AppTestCase)` in a Pest closure whose
    // `test()` parameter declares `@param-closure-this TestCase`
    // means `$this` is the subclass, and an `instanceof` check on a
    // `FirstContext|SecondContext` binding drops the other
    // alternative.  The scope only wins when it is strictly narrower;
    // otherwise it holds the lexically captured `$this` the tag is
    // there to replace.
    if let Some(bound) =
        crate::type_engine::variable::closure_resolution::find_closure_this_types(ctx)
    {
        return match from_scope {
            Some(scope) if narrows_closure_binding(&scope, &bound, class_loader) => scope,
            _ => bound,
        };
    }

    let mut this_types = if let Some(scope_types) = from_scope {
        scope_types
    } else {
        current_class
            .map(|cc| ResolvedType::from_class(cc.clone()))
            .into_iter()
            .collect()
    };

    // Inside a trait, `$this` is the class using the trait, never
    // the trait itself — see `trait_context` for what stands in for
    // it when the using class is not the one being analysed.
    if let Some(cc) = current_class
        && cc.kind == ClassLikeKind::Trait
    {
        crate::type_engine::trait_context::extend_this_with_trait_bounds(
            &mut this_types,
            cc,
            all_classes,
            class_loader,
            ctx.file.backend,
        );
    }

    this_types
}

/// Whether the scope's `$this` is strictly narrower than the classes a
/// closure binding names.
///
/// Every scope entry has to fall under some bound alternative, either as
/// that class or as a subclass of it.  The scope is narrower when one
/// entry is a proper subclass, or when it leaves out an alternative of a
/// union binding.  Members of an intersection binding share one type
/// string, so they count as a single alternative: a scope naming just one
/// of them is not narrower.
fn narrows_closure_binding(
    scope: &[ResolvedType],
    bound: &[ResolvedType],
    class_loader: &crate::class_lookup::ResolvedClassLoader<'_>,
) -> bool {
    if scope.is_empty() {
        return false;
    }
    let mut covered: Vec<&crate::php_type::PhpType> = Vec::new();
    let mut proper_subclass = false;
    for rt in scope {
        let Some(ci) = rt.class_info.as_ref() else {
            return false;
        };
        let fqn = ci.fqn();
        let mut falls_under = false;
        for b in bound {
            let Some(bound_cls) = b.class_info.as_ref() else {
                continue;
            };
            let bound_fqn = bound_cls.fqn();
            let same = fqn.eq_ignore_ascii_case(&bound_fqn);
            if !same && !crate::class_lookup::is_subclass_of(&fqn, &bound_fqn, class_loader) {
                continue;
            }
            falls_under = true;
            proper_subclass |= !same;
            if !covered.contains(&&b.type_string) {
                covered.push(&b.type_string);
            }
        }
        if !falls_under {
            return false;
        }
    }
    proper_subclass || bound.iter().any(|b| !covered.contains(&&b.type_string))
}

/// Resolve `$this` from the forward-walk scope when it carries a
/// narrowed or seeded type.
///
/// `$this` is normally resolved from the lexical `current_class`, but a
/// closure body may have a `$this` type that the enclosing class cannot
/// supply:
///
///   - `assert($this instanceof X)` inside a top-level closure (Pest
///     tests) seeds `$this` in the forward-walk scope even though there
///     is no lexical class.
///   - `instanceof` narrowing refines `$this` to a subclass.
///
/// This consults the injected `scope_var_resolver` first (used while the
/// forward walker resolves an assignment RHS), then the diagnostic scope
/// snapshot cache (used by the member-verification path after the scope
/// has been built), and finally runs the walker itself for the consumers
/// that have neither — hover, completion and go-to-definition each answer
/// a single request, so the one body walk `$this` costs there is the same
/// walk any other variable subject in the chain already pays for.
/// Returns `None` when nothing yields a type so the caller falls back to
/// `current_class`.
fn resolve_this_from_scope(ctx: &ResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    use crate::type_engine::variable::forward_walk;

    if let Some(scope_resolver) = ctx.scope_var_resolver {
        let from_scope = scope_resolver("$this");
        return (!from_scope.is_empty()).then_some(from_scope);
    }

    if forward_walk::is_diagnostic_scope_active()
        && !forward_walk::is_building_scopes()
        && forward_walk::scope_snapshots_cover(ctx.cursor_offset)
    {
        // The snapshot is authoritative here: the walker records `$this`
        // exactly when something narrowed or seeded it, so a miss means
        // there is no proof to find and re-walking would only repeat the
        // pass that built the snapshot, once per `$this->` site.  That
        // only holds where the walk actually went, which a targeted walk
        // limits to the bodies it was asked about.
        return forward_walk::lookup_diagnostic_scope("$this", ctx.cursor_offset)
            .filter(|types| !types.is_empty());
    }

    let resolved = ctx.resolve_variable_at_cursor("$this");
    (!resolved.is_empty()).then_some(resolved)
}

/// The member names a member selector can stand for.
///
/// A plain identifier names itself.  A name written as `$variable` (a
/// dynamic access such as `$obj->$name`, `$obj->{$name}()`,
/// `Class::$name()`, or their first-class callable forms) names every
/// literal string the variable can hold where it is read:
/// `$col = 'price'; $row->$col` reads `price` exactly as `$row->price`
/// would, and a `$col` of `'price'|'member_price'` reads both.
///
/// Returns no names when the variable can hold a string that is not a
/// known literal, since the member it picks is then unknown.
pub(crate) fn resolve_dynamic_member_names<'a>(
    member: &'a str,
    ctx: &ResolutionCtx<'_>,
) -> Vec<std::borrow::Cow<'a, str>> {
    use std::borrow::Cow;

    let variable = member
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .map_or(member, str::trim);
    if !variable.starts_with('$') {
        return vec![Cow::Borrowed(member)];
    }
    if !is_bare_variable(variable) {
        return Vec::new();
    }
    let mut names: Vec<Cow<'a, str>> = Vec::new();
    for rt in resolve_variable_fallback(variable, AccessKind::Arrow, ctx) {
        for ty in rt.type_string.union_members() {
            let TypeKind::Literal(literal) = ty.kind() else {
                return Vec::new();
            };
            let crate::php_type::LiteralValue::String(raw) = &**literal else {
                return Vec::new();
            };
            let Some(name) = crate::util::unescape_php_string_literal(raw) else {
                return Vec::new();
            };
            if !names.iter().any(|known| *known == name) {
                names.push(Cow::Owned(name));
            }
        }
    }
    names
}

/// Resolve a bare `$var` subject to its classes.
///
/// Resolves a variable to its classes by running the full variable
/// resolution pipeline (including narrowing from instanceof, assert,
/// ternary, and `&&` chains) and converting the result to
/// `Vec<Arc<ClassInfo>>` (dropping type-string-only entries).
pub(super) fn resolve_variable_fallback(
    var_name: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.file.all_classes;
    let class_loader = ctx.file.class_loader;

    let dummy_class;
    let effective_class = match current_class {
        Some(cc) => cc,
        None => {
            dummy_class =
                crate::class_lookup::class_context_placeholder(ctx.file.content, ctx.cursor_offset);
            &dummy_class
        }
    };

    // Guard: resolve_variable_types is designed for bare `$variable`
    // names.  SubjectExpr::Variable can carry complex expressions
    // (array access like `$arr['key']`, null coalescing, comparisons)
    // that will never match a scope entry.  Skip them to avoid wasted
    // backward scans and fallthrough noise.
    let bare_variable = is_bare_variable(var_name);
    let resolved_types = if bare_variable {
        // When a scope variable resolver is available (i.e. we are
        // inside the forward walker's scope-building pass), read the
        // variable's type directly from the in-progress ScopeState.
        // This avoids calling resolve_variable_types which would
        // trigger a full forward walk of the method body for every
        // variable access — an O(N²) blowup on files with closures.
        if let Some(scope_resolver) = ctx.scope_var_resolver {
            let prefixed = if var_name.starts_with('$') {
                var_name.to_string()
            } else {
                format!("${}", var_name)
            };
            scope_resolver(&prefixed)
        } else {
            crate::type_engine::variable::resolution::resolve_variable_types(VarQuery {
                current_class: Some(effective_class),
                ..ctx.var_query(var_name)
            })
        }
    } else {
        vec![]
    };

    // ── `class-string<T>` unwrapping for `$var::` access ────────
    // When the variable holds a class name (`$cls = Foo::class`, a
    // `@param class-string<BackedEnum> $class` annotation, a string
    // naming a class) and the access kind is `::`, resolve the classes
    // it names so that static members are offered against them.
    if access_kind == AccessKind::DoubleColon {
        let mut class_string_results: Vec<ResolvedType> = Vec::new();
        for rt in &resolved_types {
            let classes =
                crate::type_engine::variable::class_string_resolution::classes_named_by_type(
                    &rt.type_string,
                    &effective_class.fqn(),
                    all_classes,
                    class_loader,
                );
            for cls in classes {
                ResolvedType::push_unique(&mut class_string_results, ResolvedType::from_arc(cls));
            }
        }
        if !class_string_results.is_empty() {
            return class_string_results;
        }
    }

    resolved_types
}
