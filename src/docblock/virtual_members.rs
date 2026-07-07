//! Virtual member tag extraction (`@property`, `@method`).
//!
//! This submodule handles extracting magic property and method declarations
//! from class-level PHPDoc comments:
//!
//!   - `@property Type $name` / `@property-read` / `@property-write`
//!   - `@method ReturnType methodName(ParamType $param, ...)`
//!   - `@method static ReturnType methodName(...)`

use crate::atom::AtomMap;

use mago_docblock::document::TagKind;

use super::parser::parse_docblock_for_tags;
use super::tags::sanitise_and_parse_docblock_type;
use super::types::split_type_token;
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
pub fn extract_property_tags(docblock: &str) -> Vec<(String, Option<PhpType>)> {
    let Some(info) = parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };

    const PROPERTY_KINDS: &[TagKind] = &[
        TagKind::Property,
        TagKind::PropertyRead,
        TagKind::PropertyWrite,
        TagKind::PsalmProperty,
        TagKind::PsalmPropertyRead,
        TagKind::PsalmPropertyWrite,
    ];

    let mut results = Vec::new();

    for tag in info.tags_by_kinds(PROPERTY_KINDS) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }

        // Format: @property Type $name  (or)  @property $name
        if desc.starts_with('$') {
            // No explicit type: `@property $name`
            let prop_name = desc.split_whitespace().next().unwrap_or(desc);
            let name = prop_name.strip_prefix('$').unwrap_or(prop_name);
            if name.is_empty() {
                continue;
            }
            results.push((name.to_string(), None));
            continue;
        }

        // Extract the type token, respecting `<…>` nesting so that
        // generics like `Collection<int, Model>` are treated as one unit.
        let (type_token, remainder) = split_type_token(desc);

        // Find the `$name` in the remainder.
        let prop_name = match remainder.split_whitespace().find(|t| t.starts_with('$')) {
            Some(name) => name,
            None => continue,
        };

        let name = prop_name.strip_prefix('$').unwrap_or(prop_name);
        if name.is_empty() {
            continue;
        }

        // Strip trailing punctuation that could leak from descriptions
        // (e.g. trailing `.` or `,`).  The full type string including
        // nullability is preserved.
        let type_str = type_token.trim_end_matches(['.', ',']);
        let parsed = if type_str.is_empty() {
            None
        } else {
            sanitise_and_parse_docblock_type(type_str)
        };
        results.push((name.to_string(), parsed));
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

    const METHOD_KINDS: &[TagKind] = &[TagKind::Method, TagKind::PsalmMethod];

    let mut results: Vec<MethodInfo> = Vec::new();
    // Track which method names came from vendor-prefixed tags
    // (@psalm-method / @phpstan-method) so they can override
    // bare @method tags with the same name.
    let mut vendor_names: std::collections::HashSet<crate::atom::Atom> =
        std::collections::HashSet::new();

    for tag in info.tags_by_kinds(METHOD_KINDS) {
        let desc = tag.description.trim();
        if desc.is_empty() {
            continue;
        }

        // mago-docblock joins multi-line descriptions with \n; normalise.
        let desc = desc.replace('\n', " ");
        let rest = desc.as_str();

        // Check for optional `static` keyword.
        let (is_static, rest) = if let Some(after_static) = rest.strip_prefix("static") {
            // "static" must be followed by whitespace or `(` to avoid
            // matching a method literally named "staticFoo".
            if after_static.is_empty() {
                continue;
            }
            let next_char = after_static.chars().next().unwrap();
            if next_char.is_whitespace() || next_char == '(' {
                (true, after_static.trim_start())
            } else {
                (false, rest)
            }
        } else {
            (false, rest)
        };

        // Find the method name and its parameter list.
        //
        // The method name is a bare identifier immediately followed by `(`
        // at nesting depth 0.  We must skip parenthesised type prefixes
        // like `(string|int)[]` and `(callable():string)` where the `(`
        // is part of the return type, not the parameter list.
        let Some((method_name, return_type_raw, params_str, after_params, template_str)) =
            parse_method_signature(rest)
        else {
            continue;
        };

        if method_name.is_empty() {
            continue;
        }

        // When the `static` keyword was consumed as the static modifier
        // but no return type was found before the method name AND no
        // colon return type follows, `static` was actually the return
        // type (e.g. `@method static getStatic()`).  Re-interpret it.
        let (is_static, return_type_raw) = if is_static && return_type_raw.is_none() {
            // Peek ahead: if there IS a colon return type, keep
            // is_static=true (e.g. `@method static foo(): bool`).
            let has_colon_return = after_params.trim_start().starts_with(':');
            if has_colon_return {
                (true, None)
            } else {
                (false, Some("static"))
            }
        } else {
            (is_static, return_type_raw)
        };

        // Check for colon return type syntax after the parameter list:
        //   `@method methodName(params) : ReturnType description…`
        // If a return type was already found before the method name, the
        // colon syntax is ignored (prefix syntax takes precedence).
        let return_type: Option<PhpType> = if return_type_raw.is_none() {
            // Look for `: Type` after the closing paren.
            let after = after_params.trim_start();
            if let Some(after_colon) = after.strip_prefix(':') {
                let after_colon = after_colon.trim_start();
                if !after_colon.is_empty() {
                    let (type_token, _) = split_type_token(after_colon);
                    let trimmed = type_token.trim_end_matches(['.', ',']);
                    if !trimmed.is_empty() {
                        Some(PhpType::parse(trimmed))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            return_type_raw
                .map(|s| s.trim_end_matches(['.', ',']))
                .filter(|s| !s.is_empty())
                .map(PhpType::parse)
        };

        let parameters = if params_str.is_empty() {
            Vec::new()
        } else {
            parse_method_tag_params(params_str)
        };

        let method_atom = crate::atom::atom(method_name);
        let is_vendor_tag = tag.kind == TagKind::PsalmMethod;

        // Parse method-level template parameters from `<T, U of Bound>` syntax.
        let (template_params, template_param_bounds) = if let Some(tpl) = template_str {
            parse_inline_template_params(tpl)
        } else {
            (Vec::new(), AtomMap::default())
        };

        // Compute template bindings: map template param names to the
        // parameter names that directly use them as their type.
        let template_bindings = if template_params.is_empty() {
            Vec::new()
        } else {
            let tpl_names: Vec<String> = template_params.iter().map(|a| a.to_string()).collect();
            compute_template_bindings_from_params(&parameters, &tpl_names)
        };

        results.push(MethodInfo {
            name: method_atom,
            name_offset: 0,
            parameters,
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
            is_virtual: true,
            type_assertions: Vec::new(),
            throws: Vec::new(),
            if_this_is: None,
        });

        if is_vendor_tag {
            vendor_names.insert(crate::atom::atom(method_name));
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

/// Parsed components of a `@method` tag signature:
/// (method_name, return_type_raw, params_str, after_params, template_str).
type MethodSignatureParts<'a> = (&'a str, Option<&'a str>, &'a str, &'a str, Option<&'a str>);

/// Parse the method signature from the text after the optional `static`
/// keyword.
///
/// Handles parenthesised return type prefixes like `(string|int)[]` and
/// `(callable():string)` by tracking `()` nesting depth.  The method
/// name is the bare identifier token immediately before a `(` (or `<...>(`) at
/// depth 0 that is NOT preceded by a type-like token.
fn parse_method_signature(input: &str) -> Option<MethodSignatureParts<'_>> {
    // Strategy: scan for `identifier(` or `identifier<...>(` patterns at paren depth 0.
    // The last such pattern where `identifier` looks like a method name
    // (not a type keyword like `callable`) is the method name.
    //
    // Actually, a simpler approach: use `split_type_token` to consume
    // the return type (if present), then expect `methodName(...)` or
    // `methodName<TemplateParams>(...)`.
    //
    // Template params appear between the method name and the opening
    // paren: `@method TVal doThing<TVal of mixed>(TVal $param)`

    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try to find a method name by scanning for `ident(` at depth 0.
    // We need to find the *correct* `(` — the one that starts the
    // parameter list, not one inside a type expression.
    //
    // The method name is a PHP identifier: [a-zA-Z_][a-zA-Z0-9_]*
    // immediately followed by `(`.  We scan left-to-right, tracking
    // paren depth, and look for this pattern at depth 0.

    let bytes = trimmed.as_bytes();
    let len = bytes.len();
    let mut paren_depth: i32 = 0;
    let mut i = 0;

    while i < len {
        let b = bytes[i];
        match b {
            b'(' if paren_depth == 0 => {
                // Check if preceded by `>` (closing a template param list)
                // or by an identifier directly.
                let (ident_end, template_str) = if i > 1 && bytes[i - 1] == b'>' {
                    // Walk backwards to find matching `<` at depth 0.
                    let mut angle_depth = 1i32;
                    let mut k = i - 2; // start before the `>`
                    loop {
                        if bytes[k] == b'>' {
                            angle_depth += 1;
                        } else if bytes[k] == b'<' {
                            angle_depth -= 1;
                            if angle_depth == 0 {
                                break;
                            }
                        }
                        if k == 0 {
                            break;
                        }
                        k -= 1;
                    }
                    if angle_depth == 0 {
                        // k points to `<`, template content is between k+1 and i-1
                        let tpl = trimmed[k + 1..i - 1].trim();
                        (k, Some(tpl))
                    } else {
                        (i, None)
                    }
                } else {
                    (i, None)
                };

                // Check if the text immediately before `(` (or `<`) is an identifier.
                if ident_end > 0 && is_ident_byte(bytes[ident_end - 1]) {
                    // Walk backwards to find the start of the identifier.
                    let mut id_start = ident_end - 1;
                    while id_start > 0 && is_ident_byte(bytes[id_start - 1]) {
                        id_start -= 1;
                    }
                    let ident = &trimmed[id_start..ident_end];

                    // Make sure this looks like a method name, not a type
                    // keyword.  Type keywords that can appear before `(`
                    // in type expressions: `callable`, `Closure`.
                    // However, if the ident IS the first token (id_start
                    // after trimming == 0 or only whitespace before), and
                    // there's no return type before it, it could still be
                    // a method named `callable`.  We use a heuristic:
                    // if the text before the identifier (after trimming)
                    // is empty or only whitespace, this is the method name
                    // regardless of what it's called.  Otherwise, check
                    // that it's not a type keyword embedded in a type
                    // expression.
                    let before_ident = trimmed[..id_start].trim_end();

                    // If before_ident ends with `)`, `]`, `>`, or a type
                    // char, the ident might be part of a grouped type
                    // expression.  But actually, grouped types like
                    // `(string|int)[]` don't have an ident before `(`.
                    // And `callable()` has `callable` before `(`.
                    //
                    // The key insight: `callable` and `Closure` before
                    // `(` are type constructors only when they appear
                    // INSIDE the return type portion.  If the text before
                    // the ident is non-empty, this ident is the method
                    // name only if it's NOT `callable` or `Closure` when
                    // the preceding text looks like a type.  But this
                    // gets complicated.
                    //
                    // Simpler approach: if the ident is `callable` or
                    // `Closure`, skip this `(` and continue scanning
                    // (unless there's nothing before the ident, meaning
                    // the method is literally named `callable`).
                    if (ident == "callable" || ident == "Closure") && !before_ident.is_empty() {
                        // This is a callable/Closure type expression
                        // inside the return type.  Skip past the matching
                        // closing paren.
                        paren_depth += 1;
                        i += 1;
                        continue;
                    }

                    // Found the method name.  Now extract the parts.
                    let return_type_raw = if before_ident.is_empty() {
                        None
                    } else {
                        Some(before_ident)
                    };

                    // Find the matching closing paren for the parameter list.
                    let params_start = i + 1;
                    let mut depth = 1i32;
                    let mut j = params_start;
                    while j < len && depth > 0 {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                    // j is now one past the closing paren (or end of string).
                    let params_end = j - 1; // index of closing paren
                    let params_str = if params_start < params_end {
                        trimmed[params_start..params_end].trim()
                    } else {
                        ""
                    };
                    let after_params = if j < len { &trimmed[j..] } else { "" };

                    return Some((
                        ident,
                        return_type_raw,
                        params_str,
                        after_params,
                        template_str,
                    ));
                } else {
                    // `(` at depth 0 but not preceded by an identifier
                    // (and not preceded by a `>` with a valid template block).
                    // This is a grouped type like `(string|int)[]`.
                    // Track depth and continue.
                    paren_depth += 1;
                    i += 1;
                    continue;
                }
            }
            b'(' => {
                paren_depth += 1;
            }
            b')' => {
                paren_depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Returns `true` if the byte is valid in a PHP identifier
/// (`[a-zA-Z0-9_]`).
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Parse the parameter list from a `@method` tag.
///
/// Handles formats like:
///   - `string $abstract, callable():mixed $mockDefinition = null`
///   - `array<string, mixed> $data, string $connection = null`
///
/// Splits on commas while respecting `<>` and `()` nesting.
fn parse_method_tag_params(params_str: &str) -> Vec<ParameterInfo> {
    let parts = split_params(params_str);
    let mut result = Vec::new();

    for part in &parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Check for default value: ` = ...` after the variable name.
        // We look for the last `$` to find the variable name, then check
        // if `=` follows.
        let has_default = part.contains('=');

        // Check for variadic `...`
        let is_variadic = part.contains("...");

        // Find the parameter name (token starting with `$`).
        // Scan tokens right-to-left to find the `$name` token (it may be
        // followed by `= default`).
        let dollar_pos = part.rfind('$');
        let (parsed_type, param_name) = if let Some(dp) = dollar_pos {
            let name_and_rest = &part[dp..];
            // The name ends at whitespace, `=`, `)`, or end of string.
            let name_end = name_and_rest
                .find(|c: char| c.is_whitespace() || c == '=' || c == ')')
                .unwrap_or(name_and_rest.len());
            let name = &name_and_rest[..name_end];

            let before = part[..dp].trim().trim_end_matches("...");
            let parsed_type = if before.is_empty() {
                None
            } else {
                let trimmed = before.trim_end_matches(['.', ',']);
                if trimmed.is_empty() {
                    None
                } else {
                    Some(PhpType::parse(trimmed))
                }
            };

            (parsed_type, name.to_string())
        } else {
            // No `$` found — treat the whole thing as a name-less param.
            // This is unusual but we handle it gracefully.
            continue;
        };

        let is_required = !has_default && !is_variadic;

        result.push(ParameterInfo {
            name: crate::atom::atom(&param_name),
            is_required,
            type_hint: parsed_type.clone(),
            native_type_hint: parsed_type,
            description: None,
            default_value: None,
            is_variadic,
            is_reference: false,
            closure_this_type: None,
        });
    }

    result
}

/// Split a parameter string on commas while respecting `<>` and `()`
/// nesting so that `array<string, mixed>` is not split.
fn split_params(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth_angle = 0i32;
    let mut depth_paren = 0i32;
    let mut start = 0;

    for (i, ch) in s.char_indices() {
        match ch {
            '<' => depth_angle += 1,
            '>' => depth_angle -= 1,
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            ',' if depth_angle == 0 && depth_paren == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    // Push the last segment.
    parts.push(&s[start..]);
    parts
}

/// Parse inline template parameters from the `<...>` block of a `@method` tag.
///
/// Input example: `"TVal of mixed, U, V of \Countable"`
/// Returns a list of template param names (as `Atom`s) and a bounds map.
fn parse_inline_template_params(input: &str) -> (Vec<crate::atom::Atom>, AtomMap<PhpType>) {
    let mut params = Vec::new();
    let mut bounds = AtomMap::default();

    for segment in input.split(',') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        // Format: `Name` or `Name of Bound`
        let (name, bound) = if let Some(of_pos) = segment.find(" of ") {
            let name = segment[..of_pos].trim();
            let bound_str = segment[of_pos + 4..].trim();
            (
                name,
                if bound_str.is_empty() {
                    None
                } else {
                    Some(bound_str)
                },
            )
        } else {
            (segment, None)
        };

        // Skip if name doesn't look like a valid identifier.
        if name.is_empty() || !name.chars().next().unwrap().is_alphabetic() {
            continue;
        }

        let atom = crate::atom::atom(name);
        params.push(atom);
        if let Some(bound_str) = bound {
            bounds.insert(atom, PhpType::parse(bound_str));
        }
    }

    (params, bounds)
}

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

    // ─── parse_method_signature ─────────────────────────────────────────

    #[test]
    fn simple_no_return_type() {
        let (name, ret, params, _, tpl) = parse_method_signature("getString()").unwrap();
        assert_eq!(name, "getString");
        assert!(ret.is_none());
        assert_eq!(params, "");
        assert!(tpl.is_none());
    }

    #[test]
    fn simple_with_return_type() {
        let (name, ret, params, _, tpl) = parse_method_signature("string getString()").unwrap();
        assert_eq!(name, "getString");
        assert_eq!(ret, Some("string"));
        assert_eq!(params, "");
        assert!(tpl.is_none());
    }

    #[test]
    fn with_params() {
        let (name, ret, params, _, _) =
            parse_method_signature("void setInteger(int $integer)").unwrap();
        assert_eq!(name, "setInteger");
        assert_eq!(ret, Some("void"));
        assert_eq!(params, "int $integer");
    }

    #[test]
    fn grouped_union_array_return() {
        let (name, ret, params, _, _) =
            parse_method_signature("(string|int)[] getArray() with some text").unwrap();
        assert_eq!(name, "getArray");
        assert_eq!(ret, Some("(string|int)[]"));
        assert_eq!(params, "");
    }

    #[test]
    fn callable_return_in_parens() {
        let (name, ret, params, _, _) =
            parse_method_signature("(callable() : string) getCallable() dsa sada").unwrap();
        assert_eq!(name, "getCallable");
        assert_eq!(ret, Some("(callable() : string)"));
        assert_eq!(params, "");
    }

    #[test]
    fn colon_return_after_params() {
        let (name, ret, _, after, _) =
            parse_method_signature("getBool(string $foo)  :   bool dsa sada").unwrap();
        assert_eq!(name, "getBool");
        assert!(ret.is_none());
        assert!(after.trim_start().starts_with(':'));
    }

    #[test]
    fn callable_param_type_not_confused_with_method_name() {
        let (name, ret, params, _, _) =
            parse_method_signature("void setCallback(callable():mixed $mockDefinition = null)")
                .unwrap();
        assert_eq!(name, "setCallback");
        assert_eq!(ret, Some("void"));
        assert!(params.contains("$mockDefinition"));
    }

    #[test]
    fn template_params_after_method_name() {
        let (name, ret, params, _, tpl) =
            parse_method_signature("TVal get<TVal of mixed>(TVal $default)").unwrap();
        assert_eq!(name, "get");
        assert_eq!(ret, Some("TVal"));
        assert_eq!(params, "TVal $default");
        assert_eq!(tpl, Some("TVal of mixed"));
    }

    #[test]
    fn multiple_template_params() {
        let (name, ret, params, _, tpl) =
            parse_method_signature("TVal doThing<TKey, TVal of mixed>(TKey $key, TVal $val)")
                .unwrap();
        assert_eq!(name, "doThing");
        assert_eq!(ret, Some("TVal"));
        assert_eq!(params, "TKey $key, TVal $val");
        assert_eq!(tpl, Some("TKey, TVal of mixed"));
    }

    // ─── extract_method_tags ────────────────────────────────────────────

    fn make_docblock(lines: &[&str]) -> String {
        let mut s = String::from("/**\n");
        for line in lines {
            s.push_str(&format!(" * {}\n", line));
        }
        s.push_str(" */");
        s
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
