//! Native type hints, parameter lists and visibility modifiers, read off
//! the AST of a function, method or property declaration.

use std::collections::HashMap;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{atom, bytes_to_str};
use crate::php_type::PhpType;
use crate::types::*;

use super::{DocblockCtx, extract_language_level_type_for_param, is_param_available_for_version};

/// Convert a Mago [`Hint`] AST node directly to a [`PhpType`].
///
/// Since `Hint` only represents native PHP type syntax (no
/// generics, shapes, or callables), the mapping is straightforward.
pub(crate) fn extract_hint_type(hint: &Hint) -> PhpType {
    match hint {
        Hint::Identifier(ident) => PhpType::named(atom(bytes_to_str(ident.value()))),
        Hint::Nullable(nullable) => PhpType::nullable(extract_hint_type(nullable.hint)),
        Hint::Union(union) => {
            let mut members = Vec::new();
            collect_union_members(union.left, &mut members);
            collect_union_members(union.right, &mut members);
            PhpType::union(members)
        }
        Hint::Intersection(intersection) => {
            let mut members = Vec::new();
            collect_intersection_members(intersection.left, &mut members);
            collect_intersection_members(intersection.right, &mut members);
            PhpType::intersection(members)
        }
        Hint::Null(_) => PhpType::null(),
        Hint::True(_) => PhpType::true_(),
        Hint::False(_) => PhpType::false_(),
        Hint::Array(_) => PhpType::array(),
        Hint::Callable(_) => PhpType::callable(),
        Hint::Static(_) => PhpType::static_(),
        Hint::Self_(_) => PhpType::self_(),
        Hint::Parent(_) => PhpType::parent_(),
        Hint::Void(_) => PhpType::void(),
        Hint::Never(_) => PhpType::never(),
        Hint::Float(_) => PhpType::float(),
        Hint::Bool(_) => PhpType::bool(),
        Hint::Integer(_) => PhpType::int(),
        Hint::String(_) => PhpType::string(),
        Hint::Object(_) => PhpType::object(),
        Hint::Mixed(_) => PhpType::mixed(),
        Hint::Iterable(_) => PhpType::iterable(),
        Hint::Parenthesized(paren) => extract_hint_type(paren.hint),
    }
}

/// Recursively flatten nested `Hint::Union` nodes into a flat member list.
fn collect_union_members(hint: &Hint, members: &mut Vec<PhpType>) {
    match hint {
        Hint::Union(union) => {
            collect_union_members(union.left, members);
            collect_union_members(union.right, members);
        }
        other => members.push(extract_hint_type(other)),
    }
}

/// Recursively flatten nested `Hint::Intersection` nodes into a flat member list.
fn collect_intersection_members(hint: &Hint, members: &mut Vec<PhpType>) {
    match hint {
        Hint::Intersection(intersection) => {
            collect_intersection_members(intersection.left, members);
            collect_intersection_members(intersection.right, members);
        }
        other => members.push(extract_hint_type(other)),
    }
}

/// Extract parameter information from a method's parameter list.
///
/// When `content` is provided, default value expressions are extracted
/// from the source text using AST span offsets.  Pass `None` when the
/// source text is not available (the `default_value` field will be `None`
/// for every parameter in that case).
///
/// When `php_version` is `Some`, parameters annotated with
/// `#[PhpStormStubsElementAvailable]` whose version range excludes the
/// target version are filtered out.  When `None`, all parameters are
/// included.
pub(crate) fn extract_parameters(
    parameter_list: &FunctionLikeParameterList,
    content: Option<&str>,
    php_version: Option<PhpVersion>,
    doc_ctx: Option<&DocblockCtx<'_>>,
) -> Vec<ParameterInfo> {
    parameter_list
        .parameters
        .iter()
        .filter(|param| {
            // When a PHP version is configured, skip parameters that are
            // not available for that version.
            if let Some(ver) = php_version
                && let Some(ctx) = doc_ctx
            {
                is_param_available_for_version(param, ctx, ver)
            } else {
                true
            }
        })
        .map(|param| {
            let name = atom(bytes_to_str(param.variable.name));
            let is_variadic = param.ellipsis.is_some();
            let is_reference = param.ampersand.is_some();
            let has_default = param.default_value.is_some();
            let is_required = !has_default && !is_variadic;

            let native_type_hint = param.hint.as_ref().map(|h| extract_hint_type(h));

            // Check for a #[LanguageLevelTypeAware] override on the
            // parameter.  When present, it replaces the native type hint
            // with the version-appropriate type string.
            let mut type_hint = if let Some(ver) = php_version
                && let Some(ctx) = doc_ctx
            {
                extract_language_level_type_for_param(param, ctx, ver)
                    .or_else(|| native_type_hint.clone())
            } else {
                native_type_hint.clone()
            };

            // A parameter with a literal `null` default accepts null
            // regardless of its type hint. This covers the pre-8.4
            // implicit-nullable form `Type $x = null`, which PHP treats
            // as `?Type`. Fold null into the effective type so callers
            // passing null are not flagged. Re-applied after any docblock
            // `@param` merge overwrites `type_hint` (see the callers).
            if param_default_is_null(param) {
                type_hint = type_hint.map(PhpType::or_null);
            }

            let default_value = content.and_then(|src| {
                let dv = param.default_value.as_ref()?;
                let span = dv.value.span();
                let start = span.start.offset as usize;
                let end = span.end.offset as usize;
                let raw = src.get(start..end)?.trim().to_string();
                // Resolve `ClassName::class` to its FQN so that
                // downstream template substitution can find the class.
                if let Some(class_name) = raw.strip_suffix("::class") {
                    Some(resolve_default_class_name(class_name, doc_ctx))
                } else {
                    Some(raw)
                }
            });

            ParameterInfo {
                name,
                is_required,
                native_type_hint: native_type_hint.clone(),
                type_hint,
                description: None,
                default_value,
                is_variadic,
                is_reference,
                closure_this_type: None,
                param_out_type: None,
                callable_invocation: None,
            }
        })
        .collect()
}

/// Whether a parameter's default value is the literal `null`.
///
/// Used to detect the implicit-nullable form `Type $x = null`, which PHP
/// treats as `?Type` (mandatory before 8.4, still permitted after).
pub(crate) fn param_default_is_null(
    param: &function_like::parameter::FunctionLikeParameter<'_>,
) -> bool {
    let Some(dv) = param.default_value.as_ref() else {
        return false;
    };
    match dv.value {
        Expression::Literal(Literal::Null(_)) => true,
        // `Null`, `NULL` and `\null` are the same constant.
        Expression::ConstantAccess(ca) => {
            let name = bytes_to_str(ca.name.value());
            name.strip_prefix('\\')
                .unwrap_or(name)
                .eq_ignore_ascii_case("null")
        }
        _ => false,
    }
}

/// Rewrite a parameter default `Foo::class` (given as `name`, the part
/// before `::class`) to `\Fully\Qualified\Foo::class` using the file's
/// use-map and namespace from [`DocblockCtx`].
///
/// The default is read back as source text at the call site, so the result
/// keeps a leading `\`: an unprefixed `Vendor\Foo` would be qualified with
/// the caller's namespace.  Without imports or a namespace to read it
/// against (the class parse of `update_ast` passes neither) the default is
/// left as written, for the qualification that follows the parse
/// ([`qualify_param_default`]) to settle.
fn resolve_default_class_name(name: &str, doc_ctx: Option<&DocblockCtx<'_>>) -> String {
    match doc_ctx {
        Some(ctx) if !ctx.use_map.is_empty() || ctx.namespace.is_some() => {
            qualify_class_default(name, &ctx.use_map, &ctx.namespace)
        }
        _ => format!("{name}::class"),
    }
}

/// Qualify a `Foo::class` default of `param` written against `use_map` and
/// `namespace`; see [`resolve_default_class_name`].
pub(crate) fn qualify_param_default(
    param: &mut ParameterInfo,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) {
    if let Some(default) = &param.default_value
        && let Some(name) = default.strip_suffix("::class")
        && !name.starts_with('\\')
    {
        param.default_value = Some(qualify_class_default(name, use_map, namespace));
    }
}

/// Rewrite a `self::class` or `parent::class` default of `param` to the
/// class it names in the declaration, `self_fqn` or `parent_fqn`.  The
/// call site reads the default as text, where the keyword would name the
/// caller's class.  A trait passes `None` for both: there the keywords name
/// whichever class uses it.
pub(crate) fn qualify_param_default_keyword(
    param: &mut ParameterInfo,
    self_fqn: Option<&str>,
    parent_fqn: Option<&str>,
) {
    let Some(name) = param
        .default_value
        .as_deref()
        .and_then(|d| d.strip_suffix("::class"))
    else {
        return;
    };
    let target = if name.eq_ignore_ascii_case("self") {
        self_fqn
    } else if name.eq_ignore_ascii_case("parent") {
        parent_fqn
    } else {
        None
    };
    if let Some(fqn) = target {
        param.default_value = Some(format!("\\{}::class", fqn.trim_start_matches('\\')));
    }
}

/// `\Fully\Qualified\Foo::class` for a `Foo::class` default written against
/// `use_map` and `namespace`; see [`resolve_default_class_name`].  `self`,
/// `static` and `parent` are left as written.
fn qualify_class_default(
    name: &str,
    use_map: &HashMap<String, String>,
    namespace: &Option<String>,
) -> String {
    if name.starts_with('\\') || crate::class_lookup::is_class_keyword(name) {
        return format!("{name}::class");
    }
    let fqn = crate::names::resolve_to_fqn(name, use_map, namespace);
    format!("\\{fqn}::class")
}

/// Extract visibility from a set of modifiers.
/// Defaults to `Public` if no visibility modifier is present.
pub(crate) fn extract_visibility<'a>(
    modifiers: impl Iterator<Item = &'a Modifier<'a>>,
) -> Visibility {
    for m in modifiers {
        if m.is_private() {
            return Visibility::Private;
        }
        if m.is_protected() {
            return Visibility::Protected;
        }
        if m.is_public() {
            return Visibility::Public;
        }
    }
    Visibility::Public
}
