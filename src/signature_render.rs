//! Rendering a method's signature as PHP source text.
//!
//! Shared by the code actions that generate method stubs or interface
//! declarations and by override completion, which all spell a signature
//! the same way: native hints shortened against the file's imports, and
//! docblock-only types kept out of the native slots.

use std::collections::HashMap;

use crate::atom::Atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, MethodInfo, ParameterInfo};

/// Format the parameter list for a method stub.
pub(crate) fn format_params(
    method: &MethodInfo,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let mut parts = Vec::new();

    for param in &method.parameters {
        let mut s = String::new();

        // Type hint — only the native PHP signature.  Do not fall back to
        // docblock `@param` types: parameter types are contravariant, so
        // promoting a docblock type (e.g. `string`) onto an untyped parent
        // parameter would illegally narrow the override.
        if let Some(ref hint) = param.native_type_hint {
            let shortened = shorten_php_type_direct(hint, use_map, file_namespace);
            if !shortened.is_empty() {
                s.push_str(&shortened);
                s.push(' ');
            }
        }

        if param.is_reference {
            s.push('&');
        }
        if param.is_variadic {
            s.push_str("...");
        }

        // Parser stores names without `$`; virtual/docblock params may
        // already include it.  Always emit a single leading `$`.
        let pname = param.name.as_str();
        if !pname.starts_with('$') {
            s.push('$');
        }
        s.push_str(pname);

        if let Some(ref default) = param.default_value {
            s.push_str(" = ");
            s.push_str(default);
        }

        parts.push(s);
    }

    parts.join(", ")
}

/// Returns `true` if `ty` can be written as a native PHP return type hint.
///
/// Docblock-only types (generics, array shapes, callables with signatures,
/// conditional types, template variables, etc.) must not be emitted as
/// native hints in generated method stubs.
fn is_valid_native_hint(ty: &PhpType, template_params: &[Atom]) -> bool {
    match ty.kind() {
        // Plain named types and their nullable wrappers are always valid —
        // except variable references like `$this` and the method's own
        // `@template` params, which only exist in PHPDoc.  A bare `T`
        // emitted as a hint reads to PHP as a class named `T`.
        TypeKind::Named(n) => {
            !n.starts_with('$') && !template_params.iter().any(|t| t.as_str() == n.as_str())
        }
        TypeKind::Nullable(inner) => is_valid_native_hint(inner, template_params),
        // Union types are valid only when every member is valid (PHP 8+
        // union return types like `int|string|null` are legal).
        TypeKind::Union(members) => members
            .iter()
            .all(|m| is_valid_native_hint(m, template_params)),
        // Intersection types (`A&B`) are valid PHP 8.1+ hints.
        TypeKind::Intersection(members) => members
            .iter()
            .all(|m| is_valid_native_hint(m, template_params)),
        // Everything else is a docblock-only construct and must not be
        // used as a native return type hint.
        _ => false,
    }
}

/// Format the return type hint for a method stub.
pub(crate) fn format_return_type(
    method: &MethodInfo,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    // Prefer native return type (the actual PHP source-level type hint).
    if let Some(ref native) = method.native_return_type {
        let shortened = shorten_php_type_direct(native, use_map, file_namespace);
        if !shortened.is_empty() {
            return format!(": {}", shortened);
        }
    }

    // Fall back to the docblock return type, but only when it is valid PHP
    // syntax. Docblock types may contain generic annotations (e.g.
    // `array<TKey, TValue>`) that are not legal as native return type hints.
    if let Some(ref ret) = method.return_type {
        // `@return $this` has no native spelling; `static` is the hint PHP
        // itself uses for fluent returns.
        let ret = replace_this_with_static(ret);
        let shortened = shorten_php_type_direct(&ret, use_map, file_namespace);
        if is_valid_native_hint(&ret, method.template_params()) && !shortened.is_empty() {
            return format!(": {}", shortened);
        }
    }

    String::new()
}

/// Whether the hint [`format_return_type`] generates expresses `method`'s
/// documented return type in full.
///
/// `false` when the hint is dropped entirely (a docblock-only type such as
/// a generic, an array shape, or a `@template` param) and when it is a
/// lossy stand-in (`@return $this` becomes `: static`, which no longer
/// promises the same instance back).  Both are cases where a generated
/// override has to restate the documented type in a docblock.
pub(crate) fn native_hint_expresses_return_type(method: &MethodInfo) -> bool {
    let Some(ret) = method.return_type.as_ref() else {
        return true;
    };
    if let Some(native) = method.native_return_type.as_ref() {
        return native == ret;
    }
    replace_this_with_static(ret) == *ret && is_valid_native_hint(ret, method.template_params())
}

/// The effective type of the native hint a generated signature carries for
/// `param`.
///
/// A literal `null` default makes a native hint implicitly nullable, and
/// the parser folds that into the effective `type_hint`.  The generated
/// signature carries the default along with the hint, so `string $a = null`
/// already says everything `@param ?string $a` would.
pub(crate) fn native_param_hint(param: &ParameterInfo) -> Option<PhpType> {
    let native = param.native_type_hint.clone()?;
    if param
        .default_value
        .as_deref()
        .is_some_and(|d| d.eq_ignore_ascii_case("null"))
    {
        return Some(native.or_null());
    }
    Some(native)
}

/// Replace every `$this` in the type tree with `static`, the closest
/// native return type hint.
fn replace_this_with_static(ty: &PhpType) -> PhpType {
    if !ty.contains_self_ref() {
        return ty.clone();
    }
    let subs = HashMap::from([("$this".to_string(), PhpType::static_())]);
    ty.substitute(&subs)
}

/// Shorten a fully-qualified type name using the file's use-map and
/// namespace so that generated stubs match the file's import style.
///
/// For example, if the file has `use App\Models\User;`, then
/// `App\Models\User` becomes `User`.  If the class is in the same
/// namespace, the namespace prefix is dropped.
#[cfg(test)]
fn shorten_type(
    type_str: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let parsed = PhpType::parse(type_str);
    parsed
        .resolve_names(&|name| crate::names::shorten_from_fqn(name, use_map, file_namespace))
        .to_string()
}

/// Shorten a [`PhpType`] directly, without round-tripping through a string.
fn shorten_php_type_direct(
    ty: &PhpType,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    ty.resolve_names(&|name| crate::names::shorten_from_fqn(name, use_map, file_namespace))
        .to_string()
}

/// Shorten a single named type string.
pub(crate) fn shorten_single_type(
    type_str: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    crate::names::shorten_from_fqn(type_str, use_map, file_namespace)
}

/// Detect the indentation level used inside a class body.
///
/// Looks at the first method or property in the class to determine the
/// indent string.  Falls back to four spaces.
pub(crate) fn detect_class_indent(content: &str, class: &ClassInfo) -> String {
    // Look at the line where the class opening brace is and use the
    // next non-empty line's indentation as the member indentation.
    let brace_offset = class.start_offset as usize;
    if brace_offset < content.len() {
        let after_brace = &content[brace_offset..];
        for line in after_brace.lines().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            if !indent.is_empty() {
                return indent;
            }
        }
    }

    "    ".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MethodExtras;

    // ── shorten_type tests ──────────────────────────────────────────────

    #[test]
    fn shorten_type_with_use_map() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = Some("App\\Http\\Controllers".to_string());

        assert_eq!(shorten_type("App\\Models\\User", &use_map, &ns), "User");
    }

    #[test]
    fn shorten_type_same_namespace() {
        let use_map = HashMap::new();
        let ns = Some("App\\Models".to_string());

        assert_eq!(shorten_type("App\\Models\\User", &use_map, &ns), "User");
    }

    #[test]
    fn shorten_type_nullable() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = None;

        assert_eq!(shorten_type("?App\\Models\\User", &use_map, &ns), "?User");
    }

    #[test]
    fn shorten_type_union() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = None;

        assert_eq!(
            shorten_type("App\\Models\\User|null", &use_map, &ns),
            "User|null"
        );
    }

    #[test]
    fn shorten_scalar_types_unchanged() {
        let use_map = HashMap::new();
        let ns = None;

        assert_eq!(shorten_type("string", &use_map, &ns), "string");
        assert_eq!(shorten_type("int", &use_map, &ns), "int");
        assert_eq!(shorten_type("bool", &use_map, &ns), "bool");
        assert_eq!(shorten_type("void", &use_map, &ns), "void");
    }

    // ── format_params tests ─────────────────────────────────────────────

    #[test]
    fn format_params_basic() {
        let method = MethodInfo {
            parameters: vec![
                ParameterInfo {
                    name: crate::atom::atom("$name"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("string")),
                    native_type_hint: Some(PhpType::parse("string")),
                    description: None,
                    default_value: None,
                    is_variadic: false,
                    is_reference: false,
                    closure_this_type: None,
                    param_out_type: None,
                    callable_invocation: None,
                },
                ParameterInfo {
                    name: crate::atom::atom("$age"),
                    is_required: false,
                    type_hint: Some(PhpType::parse("int")),
                    native_type_hint: Some(PhpType::parse("int")),
                    description: None,
                    default_value: Some("0".to_string()),
                    is_variadic: false,
                    is_reference: false,
                    closure_this_type: None,
                    param_out_type: None,
                    callable_invocation: None,
                },
            ]
            .into(),
            ..MethodInfo::virtual_method("test", None)
        };

        let result = format_params(&method, &HashMap::new(), &None);
        assert_eq!(result, "string $name, int $age = 0");
    }

    #[test]
    fn format_params_adds_dollar_when_name_has_none() {
        // Real parsed methods store names without `$`.
        let method = MethodInfo {
            parameters: vec![ParameterInfo {
                name: crate::atom::atom("key"),
                is_required: true,
                type_hint: Some(PhpType::parse("string")),
                native_type_hint: None,
                description: None,
                default_value: None,
                is_variadic: false,
                is_reference: false,
                closure_this_type: None,
                param_out_type: None,
                callable_invocation: None,
            }]
            .into(),
            ..MethodInfo::virtual_method("getAttribute", None)
        };
        let result = format_params(&method, &HashMap::new(), &None);
        // Docblock-only type must not be promoted (contravariance).
        assert_eq!(result, "$key");
    }

    #[test]
    fn format_params_variadic_and_reference() {
        let method = MethodInfo {
            parameters: vec![
                ParameterInfo {
                    name: crate::atom::atom("$items"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("string")),
                    native_type_hint: Some(PhpType::parse("string")),
                    description: None,
                    default_value: None,
                    is_variadic: true,
                    is_reference: false,
                    closure_this_type: None,
                    param_out_type: None,
                    callable_invocation: None,
                },
                ParameterInfo {
                    name: crate::atom::atom("$out"),
                    is_required: true,
                    type_hint: Some(PhpType::parse("array")),
                    native_type_hint: Some(PhpType::parse("array")),
                    description: None,
                    default_value: None,
                    is_variadic: false,
                    is_reference: true,
                    closure_this_type: None,
                    param_out_type: None,
                    callable_invocation: None,
                },
            ]
            .into(),
            ..MethodInfo::virtual_method("test", None)
        };

        let result = format_params(&method, &HashMap::new(), &None);
        assert_eq!(result, "string ...$items, array &$out");
    }

    // ── format_return_type tests ────────────────────────────────────────

    #[test]
    fn format_return_type_with_native() {
        let method = MethodInfo {
            native_return_type: Some(PhpType::parse("string")),
            return_type: Some(PhpType::parse("string")),
            ..MethodInfo::virtual_method("test", Some("string"))
        };

        assert_eq!(
            format_return_type(&method, &HashMap::new(), &None),
            ": string"
        );
    }

    #[test]
    fn format_return_type_void() {
        let method = MethodInfo {
            native_return_type: Some(PhpType::parse("void")),
            ..MethodInfo::virtual_method("test", Some("void"))
        };

        assert_eq!(
            format_return_type(&method, &HashMap::new(), &None),
            ": void"
        );
    }

    #[test]
    fn format_return_type_this_becomes_static() {
        // `@return $this` with no native hint: `$this` is PHPDoc-only,
        // the native spelling of a fluent return is `static`.
        let method = MethodInfo {
            native_return_type: None,
            return_type: Some(PhpType::parse("$this")),
            ..MethodInfo::virtual_method("test", Some("$this"))
        };

        assert_eq!(
            format_return_type(&method, &HashMap::new(), &None),
            ": static"
        );
    }

    #[test]
    fn format_return_type_template_param_is_omitted() {
        // `@return T` names a template param, not a class.  Emitting it as
        // a native hint declares a return of the nonexistent class `T`.
        let method = MethodInfo {
            native_return_type: None,
            return_type: Some(PhpType::parse("T")),
            extras: MethodExtras {
                template_params: vec![crate::atom::atom("T")],
                ..Default::default()
            }
            .into_option(),
            ..MethodInfo::virtual_method("test", Some("T"))
        };

        assert_eq!(format_return_type(&method, &HashMap::new(), &None), "");
        assert!(!native_hint_expresses_return_type(&method));
    }

    #[test]
    fn format_return_type_nullable_this_becomes_nullable_static() {
        let method = MethodInfo {
            native_return_type: None,
            return_type: Some(PhpType::parse("$this|null")),
            ..MethodInfo::virtual_method("test", None)
        };

        assert_eq!(
            format_return_type(&method, &HashMap::new(), &None),
            ": static|null"
        );
    }

    #[test]
    fn format_return_type_none() {
        let method = MethodInfo {
            native_return_type: None,
            return_type: None,
            ..MethodInfo::virtual_method("test", None)
        };

        assert_eq!(format_return_type(&method, &HashMap::new(), &None), "");
    }

    // ── detect_class_indent tests ───────────────────────────────────────

    #[test]
    fn detect_indent_from_class_body() {
        let content = "<?php\nclass Foo {\n    public function bar() {}\n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        assert_eq!(detect_class_indent(content, &class), "    ");
    }

    #[test]
    fn detect_indent_tabs() {
        let content = "<?php\nclass Foo {\n\tpublic function bar() {}\n}\n";
        let class = ClassInfo {
            name: crate::atom::atom("Foo"),
            start_offset: content.find('{').unwrap() as u32,
            end_offset: content.rfind('}').unwrap() as u32 + 1,
            ..Default::default()
        };

        assert_eq!(detect_class_indent(content, &class), "\t");
    }

    #[test]
    fn shorten_type_generic_with_nested_union() {
        let mut use_map = HashMap::new();
        use_map.insert("User".to_string(), "App\\Models\\User".to_string());
        let ns = None;

        // The `|` inside `<…>` must NOT be treated as a union separator.
        assert_eq!(
            shorten_type("Collection<App\\Models\\User|null>", &use_map, &ns),
            "Collection<User|null>"
        );
    }
}
