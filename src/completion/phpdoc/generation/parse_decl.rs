//! Classification and parsing of the PHP declaration text that follows
//! a `/**` trigger: what kind of symbol it is, and its parameters,
//! type hints, return type, and supertypes.

use std::sync::Arc;

use tower_lsp::lsp_types::Position;

use crate::completion::phpdoc::context::{DocblockContext, SymbolInfo};
use crate::completion::phpdoc::helpers::{find_keyword_pos, find_matching_paren, split_params};
use crate::completion::source::comment_position::position_to_byte_offset;
use crate::php_type::PhpType;
use crate::types::ClassInfo;

/// Classify the PHP symbol from the first meaningful tokens after the
/// trigger.
pub(super) fn classify_declaration(text: &str) -> DocblockContext {
    let mut tokens = Vec::new();
    let mut attr_depth = 0i32;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip lines that look like docblock continuation (shouldn't
        // happen after our trigger, but be safe).
        if trimmed.starts_with('*') || trimmed.starts_with("/**") {
            continue;
        }
        // Skip PHP 8 attribute lines (#[...]).  Track bracket nesting
        // depth so that array literals inside attributes (e.g.
        // `#[Route(methods: ['GET'])]`) don't prematurely end tracking.
        if attr_depth > 0 || trimmed.starts_with("#[") {
            for ch in trimmed.chars() {
                match ch {
                    '[' => attr_depth += 1,
                    ']' => attr_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }
        for word in trimmed.split_whitespace() {
            tokens.push(word.to_lowercase());
            if tokens.len() >= 8 {
                break;
            }
        }
        if tokens.len() >= 8 {
            break;
        }
    }

    if tokens.is_empty() {
        return DocblockContext::Unknown;
    }

    let mut saw_modifier = false;
    for token in &tokens {
        let t = token.as_str();
        match t {
            "function" => return DocblockContext::FunctionOrMethod,
            "class" | "interface" | "trait" | "enum" | "abstract" | "final" | "readonly" => {
                // "abstract" and "final" could precede either a class or
                // a method.  Keep scanning.
                if matches!(t, "class" | "interface" | "trait" | "enum") {
                    return DocblockContext::ClassLike;
                }
                saw_modifier = true;
            }
            "public" | "protected" | "private" | "static" | "var" => {
                saw_modifier = true;
            }
            "const" => return DocblockContext::Constant,
            _ => {
                if saw_modifier {
                    // After a visibility/static keyword, if the next
                    // token is `function`, it's a method.  Otherwise
                    // it's likely a property (e.g. `public int $x`).
                    if t == "function" {
                        return DocblockContext::FunctionOrMethod;
                    }
                    if t.starts_with('$') {
                        return DocblockContext::Property;
                    }
                    // Could be a type hint before a property.
                    continue;
                }
                // Bare `$var` without modifiers — a local variable
                // assignment (e.g. `$var = [''];`).
                if t.starts_with('$') {
                    return DocblockContext::Inline;
                }
                break;
            }
        }
    }

    if saw_modifier {
        // Saw modifiers but no clear keyword — likely a typed property.
        return DocblockContext::Property;
    }

    DocblockContext::Unknown
}

/// Parse the declaration after the trigger to extract parameter names,
/// type hints, return types, etc.
pub(super) fn parse_declaration_info(text: &str) -> SymbolInfo {
    // Reuse the existing parser from the context module, but we need
    // to work from the raw text directly.
    let mut info = SymbolInfo::default();

    // Collect the declaration — may span multiple lines until `{` or `;`.
    let mut decl = String::new();
    let mut attr_depth = 0i32;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('*') || trimmed.starts_with("/**") {
            continue;
        }
        // Skip PHP 8 attribute lines (#[...]).  Track bracket nesting
        // depth so that array literals inside attributes don't
        // prematurely end tracking.
        if attr_depth > 0 || trimmed.starts_with("#[") {
            for ch in trimmed.chars() {
                match ch {
                    '[' => attr_depth += 1,
                    ']' => attr_depth -= 1,
                    _ => {}
                }
            }
            continue;
        }
        decl.push(' ');
        decl.push_str(trimmed);
        if trimmed.contains('{') || trimmed.contains(';') {
            break;
        }
    }

    let decl = decl.trim();
    if decl.is_empty() {
        return info;
    }

    // Check if it's a function/method.
    if let Some(func_pos) = find_keyword_pos(decl, "function") {
        let after_func = &decl[func_pos + 8..].trim_start();

        // Extract the method/function name (skip leading `&` for references).
        let name_src = after_func
            .strip_prefix('&')
            .unwrap_or(after_func)
            .trim_start();
        let name: String = name_src
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            info.method_name = Some(name);
        }

        if let Some(open_paren) = after_func.find('(') {
            let after_open = &after_func[open_paren + 1..];
            if let Some(close_paren) = find_matching_paren(after_open) {
                let params_str = &after_open[..close_paren];
                info.params = parse_params(params_str);

                let after_close = &after_open[close_paren + 1..];
                info.return_type = extract_return_type_from_decl(after_close);
            }
        }
    } else if is_class_like_keyword(decl) {
        // Class-like — extract extends/implements names.
        let (extends, implements) = extract_class_supertypes(decl);
        info.extends_names = extends;
        info.implements_names = implements;
    } else {
        // Property or constant — extract type hint.
        info.type_hint = extract_property_type(decl);
        // For inline variable assignments, extract the variable name.
        if let Some(dollar) = decl.find('$') {
            let name: String = decl[dollar..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !name.is_empty() {
                info.variable_name = Some(name);
            }
        }
    }

    info
}

/// Check whether a declaration string starts with a class-like keyword
/// (class, interface, trait, enum), possibly preceded by modifiers.
fn is_class_like_keyword(decl: &str) -> bool {
    let class_keywords = ["class", "interface", "trait", "enum"];
    let modifier_keywords = ["abstract", "final", "readonly"];
    let lower = decl.to_lowercase();
    let mut rest = lower.as_str().trim();
    loop {
        let mut found = false;
        for kw in &class_keywords {
            if let Some(after) = rest.strip_prefix(*kw)
                && (after.is_empty() || after.starts_with(|c: char| c.is_whitespace()))
            {
                return true;
            }
        }
        for kw in &modifier_keywords {
            if let Some(after) = rest.strip_prefix(*kw)
                && (after.is_empty() || after.starts_with(|c: char| c.is_whitespace()))
            {
                rest = after.trim_start();
                found = true;
                break;
            }
        }
        if !found {
            break;
        }
    }
    false
}

/// Extract parent class names and interface names from a class-like
/// declaration header (e.g. `class Foo extends Bar implements Baz`).
fn extract_class_supertypes(decl: &str) -> (Vec<String>, Vec<String>) {
    let normalised: String = decl.split_whitespace().collect::<Vec<_>>().join(" ");
    // Truncate at `{` so brace-delimited bodies don't pollute names.
    let truncated = if let Some(brace) = normalised.find('{') {
        &normalised[..brace]
    } else {
        &normalised
    };
    let lower = truncated.to_lowercase();

    let mut parents = Vec::new();
    let mut interfaces = Vec::new();

    if let Some(ext_pos) = lower.find(" extends ") {
        let after = &truncated[ext_pos + 9..];
        let end = after
            .to_lowercase()
            .find(" implements ")
            .unwrap_or(after.len());
        let segment = after[..end].trim();
        for name in segment.split(',') {
            let name = name.trim();
            if !name.is_empty() {
                parents.push(name.to_string());
            }
        }
    }

    if let Some(impl_pos) = lower.find(" implements ") {
        let after = &truncated[impl_pos + 12..];
        let segment = after.trim();
        for name in segment.split(',') {
            let name = name.trim();
            if !name.is_empty() {
                interfaces.push(name.to_string());
            }
        }
    }

    (parents, interfaces)
}

/// Parse a comma-separated parameter list into `(type_hint, $name)` pairs.
fn parse_params(params_str: &str) -> Vec<(Option<PhpType>, String)> {
    if params_str.trim().is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();

    for param in split_params(params_str) {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }

        // Each param looks like: [Type] [$name] [= default]
        // or: [Type] &$name, [Type] ...$name
        let tokens: Vec<&str> = param.split_whitespace().collect();

        // Find the variable name token (starts with $, &$, or ...$).
        let mut var_name = None;
        let mut type_parts = Vec::new();

        for tok in &tokens {
            if tok.starts_with('$') || tok.starts_with("&$") || tok.starts_with("...$") {
                let name = tok.trim_start_matches('&').trim_start_matches("...");
                // Strip default value.
                let name = if let Some(eq) = name.find('=') {
                    name[..eq].trim()
                } else {
                    name
                };
                var_name = Some(name.to_string());
                break;
            }
            // Skip `=` and default values.
            if *tok == "=" {
                break;
            }
            // Skip constructor promotion modifiers.
            match tok.to_lowercase().as_str() {
                "public" | "protected" | "private" | "static" | "readonly" => continue,
                _ => {}
            }
            type_parts.push(*tok);
        }

        if let Some(name) = var_name {
            let type_hint = if type_parts.is_empty() {
                None
            } else {
                Some(PhpType::parse(&type_parts.join(" ")))
            };
            result.push((type_hint, name));
        }
    }

    result
}

/// Extract the return type from the text after the closing `)`.
fn extract_return_type_from_decl(after_close: &str) -> Option<PhpType> {
    // Look for `: Type` pattern.
    let trimmed = after_close.trim_start();
    if !trimmed.starts_with(':') {
        return None;
    }

    let after_colon = trimmed[1..].trim_start();

    // Collect everything up to `{`, `;`, or end of string.
    let mut end = after_colon.len();
    let mut depth = 0i32;
    for (i, c) in after_colon.char_indices() {
        match c {
            '(' | '<' => depth += 1,
            ')' | '>' => depth -= 1,
            '{' | ';' if depth == 0 => {
                end = i;
                break;
            }
            _ => {}
        }
    }

    let ret_type = after_colon[..end].trim();
    if ret_type.is_empty() {
        None
    } else {
        Some(PhpType::parse(ret_type))
    }
}

/// Enrich an untyped property's [`SymbolInfo::type_hint`] by looking up
/// the property in the file's parsed class data.
///
/// When a property has no native type hint or docblock, the constructor-
/// inference pass in `extract_class_like_members` may have filled in a
/// type from `$this->prop = new ClassName()` or a promoted parameter
/// default.  This function finds that inferred type and copies it into
/// `sym` so that the generated `@var` tag uses the concrete class name
/// instead of `mixed`.
///
/// The type is shortened (leading namespace segments stripped) for
/// readability in the generated docblock.
pub(super) fn enrich_property_type_from_class(
    sym: &mut SymbolInfo,
    content: &str,
    position: Position,
    local_classes: &[Arc<ClassInfo>],
) {
    // Extract the bare property name (strip the `$` prefix).
    let prop_name = sym
        .variable_name
        .as_ref()
        .and_then(|v| v.strip_prefix('$'))
        .unwrap_or("");
    if prop_name.is_empty() {
        return;
    }

    // Find the enclosing class by byte offset.
    let cursor_offset = position_to_byte_offset(content, position) as u32;
    let enclosing = local_classes
        .iter()
        .find(|cls| cls.start_offset <= cursor_offset && cursor_offset <= cls.end_offset);
    let Some(cls) = enclosing else {
        return;
    };

    // Look up the property.  Only use the type when it was inferred
    // (the native_type_hint is None — if it were set, the source-text
    // parser would already have extracted it).
    if let Some(prop) = cls.properties.iter().find(|p| p.name == prop_name)
        && prop.native_type_hint.is_none()
        && let Some(ref inferred) = prop.type_hint
    {
        sym.type_hint = Some(inferred.shorten());
    }
}

/// Extract the type hint from a property or constant declaration.
fn extract_property_type(decl: &str) -> Option<PhpType> {
    // Strip modifiers.
    let modifiers = [
        "public",
        "protected",
        "private",
        "static",
        "readonly",
        "var",
        "const",
        "final",
    ];
    let mut rest = decl;
    loop {
        rest = rest.trim_start();
        let mut found = false;
        for m in &modifiers {
            if rest.to_lowercase().starts_with(m) {
                let after = &rest[m.len()..];
                if after.is_empty() || after.starts_with(|c: char| c.is_whitespace()) {
                    rest = after;
                    found = true;
                    break;
                }
            }
        }
        if !found {
            break;
        }
    }

    let rest = rest.trim_start();

    // If the next token starts with `$`, there's no type hint.
    if rest.starts_with('$') || rest.starts_with('=') {
        return None;
    }

    // For properties the name starts with `$`, so collect until `$`.
    // For constants the name is an identifier without `$`, so the type
    // is the first whitespace-delimited token (type hints never contain
    // spaces: `int`, `?string`, `int|string`, `A&B`, `\Foo`).
    let type_str: &str = if rest.contains('$') {
        // Property: collect everything before `$`, `=`, `;`, or `{`.
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if c == '$' || c == '=' || c == ';' || c == '{' {
                end = i;
                break;
            }
        }
        rest[..end].trim()
    } else {
        // Constant: the type (if present) is the first token, and the
        // constant name is the second.  When the first token is
        // immediately followed by `=` (i.e. there is no second token
        // before `=`), the constant is untyped and the first token is
        // actually the name.
        let mut tokens = rest.split_whitespace();
        let first = tokens.next().unwrap_or("");
        let second = tokens.next().unwrap_or("");
        if second.is_empty() || second.starts_with('=') {
            // Untyped constant: `const NAME = ...`
            ""
        } else {
            // Typed constant: `const int NAME = ...`
            first.trim()
        }
    };
    if type_str.is_empty() {
        None
    } else {
        Some(PhpType::parse(type_str))
    }
}

#[cfg(test)]
#[path = "parse_decl_tests.rs"]
mod tests;
