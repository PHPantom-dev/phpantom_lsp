//! Virtual member tag extraction (`@property`, `@method`).
//!
//! This submodule handles extracting magic property and method declarations
//! from class-level PHPDoc comments:
//!
//!   - `@property Type $name` / `@property-read` / `@property-write`
//!   - `@method ReturnType methodName(ParamType $param, ...)`
//!   - `@method static ReturnType methodName(...)`

use crate::atom::{Atom, AtomMap, atom};

use super::tag_kind::TagKind;

use super::parser::{DocblockInfo, parse_docblock_for_tags};
use super::tags::sanitise_and_parse_docblock_type;
use super::type_strings::split_type_token;
use crate::php_type::PhpType;
use crate::types::{MethodInfo, ParameterInfo, Visibility};

// ─── @property Tags ─────────────────────────────────────────────────────────

/// Extract all `@property` tags from a class-level docblock.
///
/// PHPDoc `@property` tags declare magic properties that are accessible via
/// `__get` / `__set`.  The format is:
///
///   - `@property Type $name`
///   - `@property null|Type $name`
///   - `@property ?Type $name`
///   - `@property-read Type $name`
///   - `@property-write Type $name`
///
/// Returns a list of `(property_name, cleaned_type)` pairs.  The property
/// name does **not** include the `$` prefix.
pub fn extract_property_tags(docblock: &str) -> Vec<(Atom, Option<PhpType>)> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };
    extract_property_tags_from_info(&info)
}

/// [`extract_property_tags`] against an already-parsed docblock.
///
/// Class-level docblocks are parsed once during extraction, so the class
/// parser reuses that [`DocblockInfo`] instead of re-parsing the text.
pub fn extract_property_tags_from_info(info: &DocblockInfo) -> Vec<(Atom, Option<PhpType>)> {
    const PROPERTY_KINDS: &[TagKind] = &[
        TagKind::Property,
        TagKind::PropertyRead,
        TagKind::PropertyWrite,
    ];

    let mut results = Vec::new();

    for tag in info.tags_by_kinds(PROPERTY_KINDS) {
        // `@property $name` declares an untyped magic property; `@property`
        // with no variable at all declares nothing.
        let Some(variable) = tag.variable() else {
            continue;
        };
        let Some(name) = variable.strip_prefix('$').filter(|n| !n.is_empty()) else {
            continue;
        };

        let parsed = tag
            .type_text()
            .and_then(|type_text| sanitise_and_parse_docblock_type(&type_text));
        results.push((atom(name), parsed));
    }

    results
}

// ─── @method Tags ───────────────────────────────────────────────────────────

/// Extract all `@method` tags from a class-level docblock.
///
/// PHPDoc `@method` tags declare magic methods that are accessible via
/// `__call` / `__callStatic`.  The format is:
///
///   - `@method ReturnType methodName(ParamType $param, ...)`
///   - `@method static ReturnType methodName(ParamType $param, ...)`
///   - `@method methodName(ParamType $param, ...)`  (no return type)
///
/// Returns a list of `MethodInfo` structs.  Parameters are parsed with
/// type hints and default-value detection where possible.
pub fn extract_method_tags(docblock: &str) -> Vec<MethodInfo> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };
    extract_method_tags_from_info(&info)
}

/// [`extract_method_tags`] against an already-parsed docblock.
///
/// Class-level docblocks are parsed once during extraction, so the class
/// parser reuses that [`DocblockInfo`] instead of re-parsing the text.
pub fn extract_method_tags_from_info(info: &DocblockInfo) -> Vec<MethodInfo> {
    let mut results: Vec<MethodInfo> = Vec::new();
    // Track which method names came from vendor-prefixed tags
    // (@psalm-method / @phpstan-method) so they can override
    // bare @method tags with the same name.
    let mut vendor_names: std::collections::HashSet<crate::atom::Atom> =
        std::collections::HashSet::new();

    for tag in info.tags_by_kind(TagKind::Method) {
        let Some(method) = tag.value.as_method() else {
            continue;
        };
        if method.name.is_empty() {
            continue;
        }

        // The grammar models `@method Type name(…)` but not the
        // `@method name(…): Type` spelling, which leaves the `: Type` at
        // the head of the description.
        let colon_return = method
            .description
            .as_deref()
            .and_then(|desc| desc.trim_start().strip_prefix(':'))
            .map(|after_colon| split_type_token(after_colon.trim_start()).0.trim())
            .filter(|type_token| !type_token.is_empty());

        // A lone `static` before the method name is ambiguous: the grammar
        // reads it as the return type, which is right for
        // `@method static getStatic()` but wrong for
        // `@method static foo(): bool`, where the real return type follows
        // the parameter list and `static` is the modifier.
        let static_is_modifier =
            !method.is_static && method.return_type.as_deref() == Some("static");
        let (is_static, prefix_return) = match (static_is_modifier, colon_return.is_some()) {
            (true, true) => (true, None),
            _ => (method.is_static, method.return_type.as_deref()),
        };

        // The prefix form wins when both are written.
        let return_type = prefix_return.or(colon_return).map(PhpType::parse);

        let parameters: Vec<ParameterInfo> = method
            .parameters
            .iter()
            .map(|param| {
                let type_hint = param.type_text.as_deref().map(PhpType::parse);
                ParameterInfo {
                    name: atom(&param.name),
                    is_required: !param.optional && !param.variadic,
                    type_hint: type_hint.clone(),
                    native_type_hint: type_hint,
                    description: None,
                    default_value: None,
                    is_variadic: param.variadic,
                    is_reference: false,
                    closure_this_type: None,
                }
            })
            .collect();

        // Method-level `<T of Bound>` template parameters.
        let mut template_params: Vec<Atom> = Vec::with_capacity(method.templates.len());
        let mut template_param_bounds = AtomMap::default();
        for template in &method.templates {
            let name = atom(&template.name);
            template_params.push(name);
            if let Some(bound) = &template.bound {
                template_param_bounds.insert(name, PhpType::parse(bound));
            }
        }

        // Map template param names to the parameters that use them.
        let template_bindings = if template_params.is_empty() {
            Vec::new()
        } else {
            let tpl_names: Vec<String> = template_params.iter().map(|a| a.to_string()).collect();
            compute_template_bindings_from_params(&parameters, &tpl_names)
        };

        let method_atom = atom(&method.name);
        let is_vendor_tag = tag.vendor.is_some();

        results.push(MethodInfo {
            name: method_atom,
            name_offset: 0,
            parameters: parameters.into(),
            return_type,
            native_return_type: None,
            description: None,
            return_description: None,
            links: Vec::new(),
            see_refs: Vec::new(),
            is_static,
            visibility: Visibility::Public,
            conditional_return: None,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params,
            template_param_bounds,
            template_bindings,
            has_scope_attribute: false,
            is_abstract: false,
            is_final: false,
            is_virtual: true,
            is_macro: false,
            is_inferred_return: false,
            type_assertions: Vec::new(),
            throws: Vec::new(),
            if_this_is: None,
            self_out: None,
            is_pure: false,
            is_impure: false,
        });

        if is_vendor_tag {
            vendor_names.insert(method_atom);
        }
    }

    // Deduplicate: if a method name has a vendor-prefixed entry
    // (@psalm-method / @phpstan-method), remove bare @method entries
    // with the same name. Since vendor tags come after bare tags in
    // document order, keep the last occurrence for duplicated names.
    if !vendor_names.is_empty() {
        let mut seen: std::collections::HashSet<crate::atom::Atom> =
            std::collections::HashSet::new();
        // Iterate in reverse so that later (vendor) entries are kept.
        results.reverse();
        results.retain(|m| {
            if vendor_names.contains(&m.name) {
                seen.insert(m.name)
            } else {
                true
            }
        });
        results.reverse();
    }

    results
}

// ─── Internal Helpers ───────────────────────────────────────────────────────────

/// Compute template bindings from a method's parameters.
///
/// For each parameter whose type is exactly a template parameter name,
/// creates a binding `(template_name, "$param_name")`.
fn compute_template_bindings_from_params(
    parameters: &[ParameterInfo],
    template_params: &[String],
) -> Vec<(crate::atom::Atom, crate::atom::Atom)> {
    use crate::docblock::templates::collect_template_bindings;
    let mut results = Vec::new();

    for param in parameters {
        if let Some(ref ty) = param.type_hint {
            let param_name = if param.name.starts_with('$') {
                param.name.to_string()
            } else {
                format!("${}", param.name)
            };
            collect_template_bindings(ty, template_params, &param_name, &mut results);
        }
    }

    results
        .into_iter()
        .map(|(t, p)| (crate::atom::atom(&t), crate::atom::atom(&p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── extract_method_tags ────────────────────────────────────────────

    fn make_docblock(lines: &[&str]) -> String {
        let mut s = String::from("/**\n");
        for line in lines {
            s.push_str(&format!(" * {}\n", line));
        }
        s.push_str(" */");
        s
    }

    /// Parse one `@method` line and return the single method it declares.
    fn single_method(line: &str) -> MethodInfo {
        let methods = extract_method_tags(&make_docblock(&[line]));
        assert_eq!(methods.len(), 1, "expected one @method from {line:?}");
        methods.into_iter().next().unwrap()
    }

    #[test]
    fn simple_no_return_type() {
        let method = single_method("@method getString()");
        assert_eq!(method.name.as_str(), "getString");
        assert!(method.return_type.is_none());
        assert!(method.parameters.is_empty());
        assert!(method.template_params.is_empty());
    }

    #[test]
    fn simple_with_return_type() {
        let method = single_method("@method string getString()");
        assert_eq!(method.name.as_str(), "getString");
        assert_eq!(method.return_type.as_ref().unwrap().to_string(), "string");
        assert!(method.parameters.is_empty());
    }

    #[test]
    fn with_params() {
        let method = single_method("@method void setInteger(int $integer)");
        assert_eq!(method.name.as_str(), "setInteger");
        assert_eq!(method.return_type.as_ref().unwrap().to_string(), "void");
        assert_eq!(method.parameters.len(), 1);
        assert_eq!(method.parameters[0].name.as_str(), "$integer");
        assert_eq!(
            method.parameters[0].type_hint.as_ref().unwrap().to_string(),
            "int"
        );
    }

    #[test]
    fn callable_param_type_not_confused_with_method_name() {
        // The `(` of `callable():mixed` must not be mistaken for the start
        // of the parameter list.
        let method =
            single_method("@method void setCallback(callable():mixed $mockDefinition = null)");
        assert_eq!(method.name.as_str(), "setCallback");
        assert_eq!(method.return_type.as_ref().unwrap().to_string(), "void");
        assert_eq!(method.parameters.len(), 1);
        assert_eq!(method.parameters[0].name.as_str(), "$mockDefinition");
        assert!(!method.parameters[0].is_required);
    }

    #[test]
    fn template_params_after_method_name() {
        let method = single_method("@method TVal get<TVal of mixed>(TVal $default)");
        assert_eq!(method.name.as_str(), "get");
        assert_eq!(method.return_type.as_ref().unwrap().to_string(), "TVal");
        assert_eq!(
            method
                .template_params
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>(),
            ["TVal"]
        );
        assert_eq!(
            method.template_param_bounds[&atom("TVal")].to_string(),
            "mixed"
        );
        assert_eq!(method.parameters.len(), 1);
    }

    #[test]
    fn multiple_template_params() {
        let method =
            single_method("@method TVal doThing<TKey, TVal of mixed>(TKey $key, TVal $val)");
        assert_eq!(method.name.as_str(), "doThing");
        assert_eq!(
            method
                .template_params
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>(),
            ["TKey", "TVal"]
        );
        assert_eq!(method.parameters.len(), 2);
        // Both parameters bind their template directly.
        assert_eq!(method.template_bindings.len(), 2);
    }

    #[test]
    fn static_modifier_with_parenthesised_return_type() {
        let method = single_method("@method static (string|int)[] getArray() with some text");
        assert_eq!(method.name.as_str(), "getArray");
        assert!(method.is_static);
        assert!(method.return_type.is_some());

        let method = single_method("@method static (callable() : string) getCallable() dsa");
        assert_eq!(method.name.as_str(), "getCallable");
        assert!(method.is_static);
        assert!(method.return_type.is_some());
    }

    #[test]
    fn method_named_static_is_not_treated_as_a_modifier() {
        let method = single_method("@method static(int $x)");
        assert_eq!(method.name.as_str(), "static");
        assert!(!method.is_static);
        assert_eq!(method.parameters.len(), 1);
    }

    #[test]
    fn variadic_param_is_optional() {
        let method = single_method("@method void log(string $msg, mixed ...$context)");
        assert_eq!(method.parameters.len(), 2);
        assert!(method.parameters[0].is_required);
        assert!(method.parameters[1].is_variadic);
        assert!(!method.parameters[1].is_required);
    }

    #[test]
    fn colon_return_type_parsed() {
        let doc = make_docblock(&["@method getBool(string $foo)  :   bool dsa sada"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "getBool");
        assert_eq!(methods[0].return_type.as_ref().unwrap().to_string(), "bool");
        assert!(!methods[0].is_static);
    }

    #[test]
    fn grouped_union_array_parsed() {
        let doc = make_docblock(&["@method (string|int)[] getArray() with some text"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "getArray");
        assert!(methods[0].return_type.is_some());
    }

    #[test]
    fn callable_return_type_parsed() {
        let doc = make_docblock(&["@method (callable() : string) getCallable() dsa"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "getCallable");
        assert!(methods[0].return_type.is_some());
    }

    #[test]
    fn static_keyword_as_modifier_with_return_type() {
        let doc = make_docblock(&["@method static string getString() dsa"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "getString");
        assert!(methods[0].is_static);
        assert_eq!(
            methods[0].return_type.as_ref().unwrap().to_string(),
            "string"
        );
    }

    #[test]
    fn static_keyword_reinterpreted_as_return_type() {
        // `@method static getStatic()` — only one `static`, no other
        // return type → `static` is the return type, not the modifier.
        let doc = make_docblock(&["@method static getStatic()"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "getStatic");
        assert!(
            !methods[0].is_static,
            "static should be return type, not modifier"
        );
        assert!(
            methods[0].return_type.as_ref().unwrap().is_self_ref(),
            "return type should be self-referencing (static)"
        );
    }

    #[test]
    fn static_modifier_and_static_return_type() {
        // `@method static static getInstance()` — two `static` tokens.
        let doc = make_docblock(&["@method static static getInstance()"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "getInstance");
        assert!(methods[0].is_static);
        assert!(methods[0].return_type.as_ref().unwrap().is_self_ref());
    }

    #[test]
    fn static_modifier_with_colon_return() {
        // `@method static foo(): bool` — static is the modifier,
        // bool is the colon return type.
        let doc = make_docblock(&["@method static foo(): bool"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "foo");
        assert!(methods[0].is_static);
        assert_eq!(methods[0].return_type.as_ref().unwrap().to_string(), "bool");
    }

    #[test]
    fn colon_return_with_params() {
        let doc =
            make_docblock(&["@method setBool(string $foo, string|bool $bar)  :   bool dsa sada"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "setBool");
        assert_eq!(methods[0].return_type.as_ref().unwrap().to_string(), "bool");
        assert_eq!(methods[0].parameters.len(), 2);
    }

    #[test]
    fn self_and_this_return_types() {
        let doc = make_docblock(&["@method static self getSelf()", "@method $this getThis()"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 2);

        let get_self = methods
            .iter()
            .find(|m| m.name.as_str() == "getSelf")
            .unwrap();
        assert!(get_self.is_static);
        assert!(get_self.return_type.as_ref().unwrap().is_self_ref());

        let get_this = methods
            .iter()
            .find(|m| m.name.as_str() == "getThis")
            .unwrap();
        assert!(!get_this.is_static);
        assert!(get_this.return_type.as_ref().unwrap().is_self_ref());
    }

    #[test]
    fn psalm_method_tag() {
        let doc = make_docblock(&["@psalm-method string getString() dsa"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "getString");
        assert_eq!(
            methods[0].return_type.as_ref().unwrap().to_string(),
            "string"
        );
    }

    #[test]
    fn method_with_default_params() {
        let doc =
            make_docblock(&["@method void setArray(int[]|string[] $arr = [], int $foo = 5) desc"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].parameters.len(), 2);
        assert!(!methods[0].parameters[0].is_required);
        assert!(!methods[0].parameters[1].is_required);
    }

    #[test]
    fn no_return_type_no_parens_skipped() {
        let doc = make_docblock(&["@method"]);
        let methods = extract_method_tags(&doc);
        assert!(methods.is_empty());
    }

    #[test]
    fn implicit_mixed_params() {
        let doc = make_docblock(&["@method setImplicitMixed($foo)"]);
        let methods = extract_method_tags(&doc);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "setImplicitMixed");
        assert_eq!(methods[0].parameters.len(), 1);
        assert!(methods[0].parameters[0].type_hint.is_none());
    }
}
