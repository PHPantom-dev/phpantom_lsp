//! Typing a call argument from its source text: class names, literals,
//! variables, property reads and array literals, for the call-site template
//! binding and conditional return types that receive arguments as text.

use crate::Backend;
use crate::atom::atom;
use crate::class_lookup::is_self_or_static;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;

use super::return_types::{
    literal_arg_type, resolve_call_return_hint, resolve_cast_type, resolve_chain_declared_return,
    resolve_expression_to_type, resolve_literal_type, resolve_operator_type,
    resolve_static_access_type,
};

impl Backend {
    /// Resolve an argument text string to a type name.
    ///
    /// Handles common patterns:
    /// - `ClassName::class` → `ClassName`
    /// - `new ClassName(…)` → `ClassName`
    /// - `$this` / `self` / `static` → current class name
    /// - `$this->prop` → property type
    /// - `$var` → variable type via assignment scanning
    /// - `"hello"` / `'world'` → `string`
    /// - `42` / `-1` → `int`
    /// - `3.14` → `float`
    /// - `true` / `false` → `bool`
    /// - `null` → `null`
    /// - `[…]` → `array`
    /// - `EnumClass::Case` → `EnumClass`
    /// - `ClassName::CONSTANT` → constant's declared type
    pub(crate) fn resolve_arg_text_to_type(
        arg_text: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        let trimmed = arg_text.trim();

        // A closure literal or a first-class callable (`$this->build(...)`)
        // is a `Closure` with the signature it was written with, the same
        // type the AST resolver gives it, not the result of calling it.
        if let Some(params) =
            crate::completion::source::helpers::extract_closure_params_from_text(trimmed)
        {
            let params = params
                .into_iter()
                .map(|(_, hint)| crate::php_type::CallableParam {
                    type_hint: hint
                        .map(|h| ctx.written_names().resolve_hint(&h))
                        .unwrap_or_else(PhpType::mixed),
                    optional: false,
                    variadic: false,
                })
                .collect();
            let ret = Self::infer_closure_return_type(trimmed, ctx);
            return Some(PhpType::callable_spec("Closure", params, ret));
        }
        if trimmed.ends_with("(...)") {
            let ret = crate::completion::source::helpers::resolve_first_class_callable_return_type(
                trimmed, ctx,
            );
            return Some(PhpType::callable_spec("Closure", Vec::new(), ret));
        }

        // ── Literal values ──────────────────────────────────────
        // An array literal carries the shape of what it holds, so a
        // template or conditional return keyed on that shape sees it the
        // same way it would through an assignment.
        if let Some(ty) = crate::type_engine::variable::rhs_resolution::infer_array_literal_type(
            trimmed,
            &|element| Self::resolve_arg_text_to_type(element, ctx),
        ) {
            return Some(ty);
        }
        if let Some(ty) = resolve_literal_type(trimmed) {
            return Some(ty);
        }

        // ── Casts ───────────────────────────────────────────────
        // `(string) $customer->id` is a `string` whatever the property
        // resolves to, so the cast answers before the operand is read.
        if let Some(ty) = resolve_cast_type(trimmed) {
            return Some(ty);
        }

        // ClassName::class → class-string<ClassName>
        //
        // The magic `::class` constant yields the fully-qualified class
        // name as a `class-string<T>`, mirroring the general expression
        // resolver (`resolve_rhs_property_access`).  Keeping the wrapper
        // here means a template param bound directly from a `::class`
        // argument (`@param T $x`) infers `class-string<T>` rather than
        // the bare class, matching the argument's actual type.  The
        // `class-string<T>` unwrapping paths (ClassStringInner and the
        // class-string generic wrapper) strip the wrapper back off when
        // they need the bare class.
        if let Some(name) = trimmed.strip_suffix("::class")
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
        {
            // self::class / static::class / parent::class resolve relative
            // to the class at the call site.
            let class_named = if is_self_or_static(name) {
                ctx.current_class.map(|c| PhpType::named(c.fqn()))
            } else if name.eq_ignore_ascii_case("parent") {
                ctx.current_class
                    .and_then(|c| c.parent_class.as_ref())
                    .map(|p| PhpType::named(atom(p.as_ref())))
            } else {
                Some(PhpType::named(atom(
                    &ctx.written_names().resolve_class(name),
                )))
            };
            return class_named.map(|n| PhpType::class_string(Some(n)));
        }

        // Global constant access: `PHP_VERSION`, `PHP_EOL`, etc.
        //
        // A bare identifier that isn't a keyword, a `::class`/enum/const
        // access (handled above and below), or any other special form is a
        // global constant reference.  Ask the attached `Backend` (the same
        // source `VarResolutionCtx`'s constant loader draws from) and infer
        // the type from its value, mirroring the
        // `Expression::ConstantAccess` branch the AST-based RHS resolver
        // already has for a plain `$x = PHP_EOL;` assignment.  This path
        // takes expression *text*, with no offset to resolve a namespaced
        // name against, so only the name as written is tried.
        if !trimmed.is_empty()
            && !trimmed.starts_with('$')
            && !trimmed.contains("::")
            && !trimmed.contains("->")
            && !trimmed.contains('(')
            && !trimmed.contains('[')
            && trimmed
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
            && !is_self_or_static(trimmed)
            && !trimmed.eq_ignore_ascii_case("parent")
            && let Some(backend) = ctx.file.backend
            && let Some(ty) = match backend.lookup_global_constant(trimmed) {
                None => None,
                Some(Some(value)) => {
                    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
                        &value,
                    )
                    .or_else(|| super::folded_global_constant_type(trimmed, &value, ctx))
                }
                Some(None) => crate::resolution::unversioned_php_version_constant_type(trimmed),
            }
        {
            return Some(ty);
        }

        // When the expression contains a `->` chain (e.g.
        // `Country::DK->value`, `new Decimal($x)->toFixed(2)`),
        // skip the static-access and new-expression shortcuts —
        // they would match the prefix and ignore the chain.
        // Let `resolve_expression_to_type` handle the full chain.
        let has_arrow_chain = trimmed.contains("->");

        // ClassName::Member — enum cases and class constants.
        // Enum cases resolve to the enum type; class constants
        // resolve to the constant's declared type hint.  Of the `->`
        // chains, it answers only a case's `->name` / `->value`.
        if let Some(ty) = resolve_static_access_type(trimmed, ctx) {
            return Some(ty);
        }

        // new ClassName(…) → ClassName
        if !has_arrow_chain
            && let Some(class_name) =
                crate::completion::source::helpers::extract_new_expression_class(trimmed)
        {
            // `new $class(…)` builds whatever class the variable names.
            if class_name.starts_with('$') {
                let built = crate::type_engine::resolver::resolve_target_classes_expr(
                    &crate::type_engine::subject_expr::SubjectExpr::NewExpr { class_name },
                    crate::AccessKind::Arrow,
                    ctx,
                );
                return (!built.is_empty())
                    .then(|| crate::types::ResolvedType::types_joined(&built));
            }
            let resolved_name = ctx.written_names().resolve_class(&class_name);
            return Some(PhpType::named(atom(
                resolved_name.strip_prefix('\\').unwrap_or(&resolved_name),
            )));
        }

        // $this / self / static → current class (or preserve the keyword when asked)
        if is_self_or_static(trimmed) {
            return ctx.current_class.map(|c| {
                if ctx.preserve_static {
                    match trimmed {
                        "static" => PhpType::static_type(c.fqn()),
                        "$this" => PhpType::this_type(c.fqn()),
                        _ => PhpType::named(c.fqn()),
                    }
                } else {
                    PhpType::named(c.fqn())
                }
            });
        }

        // When preserve_static is set, try resolving method chains by
        // looking up the last method's declared return type directly.
        // This preserves $this/static and generics that the general
        // expression resolver would flatten to a bare class name.
        if ctx.preserve_static
            && trimmed.contains("->")
            && let Some(ty) = resolve_chain_declared_return(trimmed, ctx)
        {
            return Some(ty);
        }

        // Operators whose result is decided from their operands rather
        // than from source-text shape alone (`$a . $b`, `$body ?: ''`).
        // Checked before the general fallback because `SubjectExpr::parse`
        // has no notion of these operators and would otherwise misread the
        // whole expression as a single bare variable or class name.
        if let Some(ty) = resolve_operator_type(trimmed, ctx) {
            return Some(ty);
        }

        // General expression fallback: parse the argument text as a
        // SubjectExpr and try to resolve it to a type.  This handles
        // $var, $var->prop, $this->prop, $var->method(), method
        // chains, and any other expression pattern.
        if let Some(ty) = resolve_expression_to_type(trimmed, ctx) {
            return Some(ty);
        }

        // A call whose return type names no class (`$review->getRating()`
        // returning `int`) leaves the general resolver empty, because it
        // reports classes.  Read the call's return type from the same
        // resolution path so a template can still bind from it.
        if let Some(ty) = resolve_call_return_hint(trimmed, ctx) {
            return Some(ty);
        }

        // The general resolver only reports class-backed results, so a
        // property or variable holding a non-class type (`array<string,
        // Leaf>`) comes back empty.  Read the declared type directly so
        // template params can still bind from it.
        crate::type_engine::variable::rhs_resolution::resolve_arg_variable_raw_type(trimmed, ctx)
    }
}

/// Resolve the elements of an array literal argument to the type a template
/// bound through them takes.
///
/// `resolve_arg_text_to_type("[1, 2, 3]")` types the whole literal (as a
/// shape, or a bare `array` when an element cannot be typed), so callers
/// that need the element type itself (binding a template through an
/// array-like wrapper) unwrap the literal and resolve its elements.  A scalar
/// literal element stays a literal, as a scalar argument bound directly
/// does (`[1, 2]` binds `1|2`), and an empty literal has no element at all,
/// so it binds `never`.
///
/// Returns `None` when `arg_text` is not a `[...]` literal, or an element
/// (a spread, or an expression we cannot resolve) says nothing definite.
pub(crate) fn array_literal_element_type(
    arg_text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))?
        .trim();
    if inner.is_empty() {
        return Some(PhpType::never());
    }
    let mut members: Vec<PhpType> = Vec::new();
    for elem in crate::text_scan::split_top_level_commas(inner) {
        let elem = elem.trim();
        if elem.is_empty() {
            continue;
        }
        if elem.starts_with("...") {
            return None;
        }
        let value_text = match crate::text_scan::split_top_level_arrow(elem) {
            Some((_, value)) => value.trim(),
            None => elem,
        };
        let ty = literal_arg_type(value_text)
            .or_else(|| Backend::resolve_arg_text_to_type(value_text, ctx))?;
        for member in ty.union_members() {
            if !members.contains(member) {
                members.push(member.clone());
            }
        }
    }
    match members.len() {
        0 => None,
        1 => members.pop(),
        _ => Some(PhpType::union(members)),
    }
}

/// Build an `array{key: type, ...}` shape from an array literal argument's
/// own keys and values.
///
/// `resolve_arg_text_to_type("['debug' => $x]")` collapses a literal with
/// an element it cannot type to the bare `array` keyword, which erases the
/// literal's own keys. A template bound directly to the whole argument (no
/// wrapping hint to unify against) needs those keys preserved so
/// `key-of<T>`/`value-of<T>` on the bound template can still project them
/// out, so an element nothing can type is recorded as `mixed` here.
///
/// Only keyed entries are recorded — a mixed literal's positional elements
/// are dropped, matching the AST-based array literal inference in
/// `raw_type_inference.rs`. Returns `None` when `arg_text` is not a
/// `[...]`/`array(...)` literal, or none of its entries have a literal
/// string/int key.
pub(crate) fn array_literal_shape_type(arg_text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    array_literal_shape_type_with(arg_text, &|text| {
        Backend::resolve_arg_text_to_type(text, ctx)
    })
}

/// [`array_literal_shape_type`] with the values that are not scalar
/// literals resolved by `resolve_value`, for a caller that has an argument
/// resolver rather than a resolution context.
///
/// A value that is itself an array literal gets a shape of its own
/// (`[['a', 'b']]` is `array{array{'a', 'b'}}`).
pub(crate) fn array_literal_shape_type_with(
    arg_text: &str,
    resolve_value: &dyn Fn(&str) -> Option<PhpType>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .or_else(|| {
            trimmed
                .strip_prefix("array(")
                .and_then(|s| s.strip_suffix(')'))
        })?
        .trim();
    if inner.is_empty() {
        return None;
    }

    let mut entries: Vec<crate::php_type::ShapeEntry> = Vec::new();
    // The key PHP gives the next element written without one: one past the
    // largest integer key so far.  Unknown once a key we cannot read (or a
    // spread) could have been an integer.
    let mut next_index: Option<i64> = Some(0);
    for elem in crate::text_scan::split_top_level_commas(inner) {
        let elem = elem.trim();
        if elem.is_empty() {
            continue;
        }
        let (key, value_text, unkeyed) = match crate::text_scan::split_top_level_arrow(elem) {
            Some((key_text, value)) => {
                let key = literal_array_key_text(key_text.trim());
                match key.as_deref().map(str::parse::<i64>) {
                    Some(Ok(int_key)) => {
                        next_index = next_index.map(|next| next.max(int_key.saturating_add(1)));
                    }
                    Some(Err(_)) => {}
                    None => next_index = None,
                }
                (key, value.trim(), false)
            }
            None if elem.starts_with("...") => {
                next_index = None;
                (None, elem, false)
            }
            None => {
                let key = next_index.map(|index| index.to_string());
                next_index = next_index.map(|index| index.saturating_add(1));
                (key, elem, true)
            }
        };
        let Some(key) = key else {
            continue;
        };
        // Positional while it lands on the index a reader counting the
        // positional entries before it would expect, the way the AST's
        // array literal inference spells it.
        let positional_count = entries.iter().filter(|entry| entry.key.is_none()).count();
        let key = (!unkeyed || positional_count.to_string() != key).then_some(key);
        // `resolve_arg_text_to_type` widens a scalar literal to its base
        // type (`1` → `int`), which would leave `value-of<T>` over the
        // bound shape with the scalar rather than the literal the caller
        // wrote. Keep int/float/string literals precise; everything else
        // (`true`/`false`, `null`, variables, calls) resolves as before.
        let value_type =
            crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
                value_text,
            )
            .filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
            .or_else(|| array_literal_shape_type_with(value_text, resolve_value))
            .or_else(|| resolve_value(value_text))
            .unwrap_or_else(PhpType::mixed);
        entries.push(crate::php_type::ShapeEntry {
            key,
            value_type,
            optional: false,
        });
    }

    (!entries.is_empty()).then(|| PhpType::array_shape(entries))
}

/// Extract the literal key text of an array literal's key expression
/// (`'debug'`, `"verbose"`, `42`), unquoting string keys. Returns `None`
/// for any key that is not a literal (e.g. a constant or variable), since
/// those cannot be projected into a shape's key set.
fn literal_array_key_text(key_text: &str) -> Option<String> {
    if let Some(unquoted) = crate::text_scan::unquote_php_string(key_text) {
        return Some(unquoted.to_string());
    }
    let numeric = key_text.strip_prefix('-').unwrap_or(key_text);
    (!numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit()))
        .then(|| key_text.to_string())
}
