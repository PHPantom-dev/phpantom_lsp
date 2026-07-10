//! Hover formatting helpers.
//!
//! Pure functions that take data types and return formatted strings or
//! `Hover` values.  These have no dependency on `Backend` and are used
//! by the dispatch/resolution code in `hover/mod.rs`.

use tower_lsp::lsp_types::*;

use crate::docblock::parser::{DocblockInfo, parse_docblock_for_tags};
use crate::php_type::PhpType;
use crate::symbol_map::SymbolSpan;
use crate::types::*;
use crate::util::offset_to_position;

/// Convert a `SymbolSpan`'s byte offsets to an LSP `Range`.
pub(super) fn symbol_span_to_range(content: &str, symbol: &SymbolSpan) -> Range {
    Range {
        start: offset_to_position(content, symbol.start as usize),
        end: offset_to_position(content, symbol.end as usize),
    }
}

/// Create a `Hover` with Markdown content.
pub(super) fn make_hover(contents: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: contents,
        }),
        range: None,
    }
}

/// Format a deprecation message as a Markdown line for hover output.
///
/// Returns `"🪦 **deprecated**"` when the message is empty, or
/// `"🪦 **deprecated** Use foo() instead."` when a message is present.
pub(super) fn format_deprecation_line(msg: &str) -> String {
    if msg.is_empty() {
        "🪦 **deprecated**".to_string()
    } else {
        format!("🪦 **deprecated** {}", msg)
    }
}

/// Format a provenance line showing where a symbol comes from.
///
/// Returns a short Markdown line with an emoji indicator:
/// - `🟢 laravel/framework` — direct Composer dependency
/// - `🟠 phpstan/phpstan` — transitive dependency
/// - `🟣 PHP` — core/stub (built-in PHP class or extension)
/// - `None` — project code (no badge needed)
pub(super) fn format_provenance_line(
    origin: crate::ClassCompletionOrigin,
    package_name: Option<&str>,
) -> Option<String> {
    use crate::ClassCompletionOrigin;
    match origin {
        ClassCompletionOrigin::Project => None,
        ClassCompletionOrigin::CoreStub => Some("🟣 `PHP`".to_string()),
        ClassCompletionOrigin::VendorExplicit => {
            let name = package_name.unwrap_or("vendor");
            Some(format!("🟢 `{}`", name))
        }
        ClassCompletionOrigin::VendorTransitive => {
            let name = package_name.unwrap_or("vendor");
            Some(format!("🟠 `{}` *(transitive)*", name))
        }
    }
}

/// Format a visibility keyword.
pub(super) fn format_visibility(vis: Visibility) -> &'static str {
    match vis {
        Visibility::Public => "public ",
        Visibility::Protected => "protected ",
        Visibility::Private => "private ",
    }
}

/// Format a parameter list using native PHP type hints only.
///
/// Used inside `<?php` code blocks so the displayed declaration matches
/// what the actual PHP source code looks like.
pub(super) fn format_native_params(params: &[ParameterInfo]) -> String {
    format_params_inner(params, true)
}

/// Shared implementation for parameter formatting.
///
/// When `use_native` is true, uses `native_type_hint` (falling back to
/// `type_hint` when no native hint is stored — e.g. for virtual members
/// synthesised from docblocks).  Otherwise uses `type_hint` (effective).
fn format_params_inner(params: &[ParameterInfo], use_native: bool) -> String {
    params
        .iter()
        .map(|p| {
            let mut parts = Vec::new();
            let hint: Option<String> = if use_native {
                p.native_type_hint.as_ref().map(|t| t.to_string())
            } else {
                p.type_hint.as_ref().map(|t| t.to_string())
            };
            if let Some(th) = hint {
                parts.push(th);
            }
            if p.is_variadic {
                parts.push(format!("...{}", p.name));
            } else if p.is_reference {
                parts.push(format!("&{}", p.name));
            } else {
                parts.push(p.name.to_string());
            }
            let param_str = parts.join(" ");
            if !p.is_required && !p.is_variadic {
                if let Some(ref dv) = p.default_value {
                    format!("{} = {}", param_str, dv)
                } else {
                    format!("{} = ...", param_str)
                }
            } else {
                param_str
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build a `namespace Foo;\n` line for use inside PHP code blocks.
/// Returns an empty string when the namespace is global (None).
pub(super) fn namespace_line(namespace: Option<&str>) -> String {
    if let Some(ns) = namespace
        && !ns.is_empty()
        && !ns.starts_with("___")
    {
        format!("namespace {};\n", ns)
    } else {
        String::new()
    }
}

/// Build a `@var` docblock annotation when the effective type differs from
/// the native type.  Returns `None` when they are identical or when there
/// is no effective type.
pub(super) fn build_var_annotation(
    effective: Option<&PhpType>,
    native: Option<&PhpType>,
) -> Option<String> {
    let eff = effective?;
    // When there is no native type hint, `mixed` is the implicit type
    // in PHP — showing `@var mixed` would be noise.
    if native.is_none() && eff.is_mixed() {
        return None;
    }
    if let Some(n) = native
        && eff.equivalent(n)
    {
        return None;
    }
    Some(format!("@var {}", shorten_php_type(eff)))
}

/// Build a readable markdown section showing parameter and return type
/// information.
///
/// Produces output like:
///
/// ```text
/// **$callback** `(callable(TItem): TReturn)|null`
///     Callback function to run for each element.
/// **$array** `array<string|int, TItem>`
///     An array to run through the callback function.
/// **$arrays** `array<string|int, TItem>` ...
/// **return** `array<string|int, TReturn>`
///     an array containing all the elements of arr1 ...
/// ```
///
/// A parameter entry is emitted when:
///   - the effective type differs from the native type, OR
///   - the parameter has a description.
///
/// When types are the same, the type is shown alongside the description.
/// When types differ but there is no description, only the type is shown.
///
/// A return entry is emitted when:
///   - the effective return type differs from the native return type, OR
///   - there is a return description.
///
/// Returns `None` when there is nothing to show.
pub(super) fn build_param_return_section(
    params: &[ParameterInfo],
    effective_return: Option<&PhpType>,
    native_return: Option<&PhpType>,
    return_description: Option<&str>,
) -> Option<String> {
    let mut entries = Vec::new();

    for p in params {
        let type_differs = match (&p.type_hint, p.native_type_hint.as_ref()) {
            (Some(eff_type), Some(nat)) => !eff_type.equivalent(nat),
            (Some(eff_type), None) => !eff_type.is_mixed(),
            _ => false,
        };
        let has_desc = p.description.as_ref().is_some_and(|d| !d.is_empty());

        if !type_differs && !has_desc {
            continue;
        }

        let mut entry = format!("**{}**", p.name);
        if type_differs {
            if let Some(ref eff_type) = p.type_hint {
                entry.push_str(&format!(" `{}`", shorten_php_type(eff_type)));
            }
            if p.is_variadic {
                entry.push_str(" ...");
            }
            if has_desc {
                entry.push_str("  \n\u{00a0}\u{00a0}\u{00a0}\u{00a0}");
                entry.push_str(p.description.as_deref().unwrap());
            }
        } else if has_desc {
            // Types match — show description directly after the name.
            entry.push(' ');
            entry.push_str(p.description.as_deref().unwrap());
        }
        entries.push(entry);
    }

    // return entry
    let ret_type_differs = match (effective_return, native_return) {
        (Some(eff), Some(nat)) => !eff.equivalent(nat),
        (Some(eff), None) => !eff.is_mixed(),
        _ => false,
    };
    let has_ret_desc = return_description.is_some_and(|d| !d.is_empty());

    if ret_type_differs || has_ret_desc {
        let mut entry = String::from("**return**");
        if ret_type_differs {
            if let Some(eff) = effective_return {
                entry.push_str(&format!(" `{}`", shorten_php_type(eff)));
            }
            if has_ret_desc {
                entry.push_str("  \n\u{00a0}\u{00a0}\u{00a0}\u{00a0}");
                entry.push_str(return_description.unwrap());
            }
        } else if has_ret_desc {
            entry.push(' ');
            entry.push_str(return_description.unwrap());
        }
        entries.push(entry);
    }

    if entries.is_empty() {
        None
    } else {
        Some(entries.join("\n\n"))
    }
}

/// Build a PHP code block wrapping a member inside its owning class.
///
/// Produces a fenced `php` block containing:
///
///   - `<?php`
///   - `namespace Foo;` (omitted when global)
///   - `class ShortName {`
///   - `    public string $name;`
///   - `}`
pub(super) fn build_class_member_block(
    owner_name: &str,
    owner_namespace: Option<&str>,
    kind_keyword: &str,
    name_suffix: &str,
    member_line: &str,
) -> String {
    let mut body = String::new();
    let ns_line = namespace_line(owner_namespace);
    body.push_str("```php\n<?php\n");
    body.push_str(&ns_line);
    body.push_str(kind_keyword);
    body.push(' ');
    body.push_str(owner_name);
    body.push_str(name_suffix);
    body.push_str(" {\n    ");
    body.push_str(member_line);
    body.push_str("\n}\n```");
    body
}

/// Return the PHP keyword for a class-like owner.
///
/// Produces `"class"`, `"interface"`, `"trait"`, or `"enum"`.
pub(super) fn owner_kind_keyword(owner: &ClassInfo) -> &'static str {
    match owner.kind {
        ClassLikeKind::Interface => "interface",
        ClassLikeKind::Trait => "trait",
        ClassLikeKind::Enum => "enum",
        _ => "class",
    }
}

/// Return the suffix after the owner name for backed enums (e.g. `": string"`).
///
/// Returns an empty string for non-enums and unit enums.
pub(super) fn owner_name_suffix(owner: &ClassInfo) -> String {
    if let Some(ref bt) = owner.backed_type {
        format!(": {}", bt)
    } else {
        String::new()
    }
}

/// Build a PHP code block wrapping a member inside its owning class,
/// with an optional single-line `/** @var ... */` annotation above it.
///
/// Used for properties where the effective (docblock) type differs from
/// the native PHP type hint.
pub(super) fn build_class_member_block_with_var(
    owner_name: &str,
    owner_namespace: Option<&str>,
    kind_keyword: &str,
    name_suffix: &str,
    var_annotation: &Option<String>,
    member_line: &str,
) -> String {
    let mut body = String::new();
    let ns_line = namespace_line(owner_namespace);
    body.push_str("```php\n<?php\n");
    body.push_str(&ns_line);
    body.push_str(kind_keyword);
    body.push(' ');
    body.push_str(owner_name);
    body.push_str(name_suffix);
    body.push_str(" {\n");
    if let Some(annotation) = var_annotation {
        body.push_str("    /** ");
        body.push_str(annotation);
        body.push_str(" */\n");
    }
    body.push_str("    ");
    body.push_str(member_line);
    body.push_str("\n}\n```");
    body
}

/// Build hover content for a standalone function.
pub(crate) fn hover_for_function(
    func: &FunctionInfo,
    resolved_see: Option<&[ResolvedSeeRef]>,
    provenance: Option<String>,
) -> Hover {
    let native_params = format_native_params(&func.parameters);

    // Use native return type in the code block.
    let native_ret = func
        .native_return_type
        .as_ref()
        .map(|r| format!(": {}", r))
        .unwrap_or_default();

    let signature = format!("function {}({}){}", func.name, native_params, native_ret);
    let ns_line = namespace_line(func.namespace.as_deref());

    let mut lines = Vec::new();

    if let Some(ref desc) = func.description {
        lines.push(desc.clone());
    }

    if let Some(ref msg) = func.deprecation_message {
        lines.push(format_deprecation_line(msg));
    }

    for url in &func.links {
        lines.push(format!("[{}]({})", url, url));
    }

    if let Some(refs) = resolved_see {
        format_see_refs(refs, &func.links, &mut lines);
    } else {
        // Fallback: render raw @see refs without location links.
        let unresolved: Vec<ResolvedSeeRef> = func
            .see_refs
            .iter()
            .map(|raw| ResolvedSeeRef {
                raw: raw.clone(),
                location_uri: None,
            })
            .collect();
        format_see_refs(&unresolved, &func.links, &mut lines);
    }

    // Build the readable param/return section as markdown.
    if let Some(section) = build_param_return_section(
        &func.parameters,
        func.return_type.as_ref(),
        func.native_return_type.as_ref(),
        func.return_description.as_deref(),
    ) {
        lines.push(section);
    }

    // Build a clean code block with just the signature.
    let code = format!("```php\n<?php\n{}{};\n```", ns_line, signature);
    lines.push(code);

    if let Some(prov) = provenance {
        lines.push(prov);
    }

    make_hover(lines.join("\n\n"))
}

/// A `@see` reference that has been resolved to an optional file location.
///
/// When `location_uri` is `Some`, the symbol name is rendered as a
/// clickable link that opens the target file at the definition site.
pub(crate) struct ResolvedSeeRef {
    /// The raw text after `@see` (e.g. `"UnsetDemo"`,
    /// `"MyClass::method() Use this instead"`,
    /// `"https://example.com/docs"`).
    pub raw: String,
    /// File URI with line fragment (e.g. `"file:///path/to/file.php#L42"`)
    /// for symbol references that could be resolved to a definition site.
    /// `None` for URLs or unresolvable symbols.
    pub location_uri: Option<String>,
}

/// Format `@see` references as hover lines.
///
/// URL references are rendered as clickable markdown links.
/// Symbol references with a resolved location are rendered as clickable
/// file links that jump to the definition site.  Unresolved symbols are
/// rendered as inline code.
/// Each entry becomes a separate line in the hover popup.
pub(super) fn format_see_refs(
    see_refs: &[ResolvedSeeRef],
    existing_links: &[String],
    lines: &mut Vec<String>,
) {
    for entry in see_refs {
        // Split into the first token (symbol or URL) and optional description.
        let (target, description) = match entry.raw.split_once(|c: char| c.is_whitespace()) {
            Some((t, d)) => (t.trim(), Some(d.trim())),
            None => (entry.raw.as_str(), None),
        };

        let desc_suffix = description.map(|d| format!(" {}", d)).unwrap_or_default();

        if target.starts_with("http://") || target.starts_with("https://") {
            // Skip URL references that already appear as @link entries.
            if existing_links.iter().any(|l| l == target) {
                continue;
            }
            // URL reference — render as a clickable markdown link,
            // same style as @link.
            lines.push(format!("[{}]({}){}", target, target, desc_suffix));
        } else if let Some(ref uri) = entry.location_uri {
            // Symbol reference with resolved location — render as a
            // clickable link that opens the file at the definition line.
            lines.push(format!("[`{}`]({}){}", target, uri, desc_suffix));
        } else {
            // Symbol reference without a known location — inline code.
            lines.push(format!("`{}`{}", target, desc_suffix));
        }
    }
}

/// Extract the trailing description from a `@var` tag in a pre-parsed
/// [`DocblockInfo`].
///
/// Handles formats like:
///   - `@var list<Pen> The batches`       → `Some("The batches")`
///   - `@var list<Pen> $batch The batches` → `Some("The batches")`
///   - `@var list<Pen>`                    → `None`
pub(crate) fn extract_var_description_from_info(info: &DocblockInfo) -> Option<String> {
    use mago_docblock::document::TagKind;

    let tag = info.first_tag_by_kind(TagKind::Var)?;
    let desc = tag.description.trim();
    if desc.is_empty() {
        return None;
    }
    // Skip past the type token (respecting `<…>` nesting).
    let after_type = skip_type_token(desc);
    let after_type = after_type.trim_start();
    if after_type.is_empty() {
        return None;
    }
    // Skip an optional `$variable` name.
    let after_var = if after_type.starts_with('$') {
        after_type
            .split_once(|c: char| c.is_whitespace())
            .map(|(_, rest)| rest.trim_start())
            .unwrap_or("")
    } else {
        after_type
    };
    if after_var.is_empty() {
        return None;
    }
    Some(after_var.to_string())
}

/// Skip past a type token in a docblock string, respecting `<…>` nesting.
///
/// Returns the remainder of the string after the type token.
fn skip_type_token(s: &str) -> &str {
    let (_token, rest) = crate::docblock::type_strings::split_type_token(s);
    rest
}

/// Convert basic HTML markup in docblock text to Markdown equivalents.
///
/// Handles `<b>`, `<i>`, `<code>`, `<br>`, and `<p>` tags.  This is a
/// simple string-replacement pass, not a full HTML parser.
pub(crate) fn html_to_markdown(text: &str) -> String {
    text.replace("<b>", "**")
        .replace("</b>", "**")
        .replace("<i>", "*")
        .replace("</i>", "*")
        .replace("<code>", "`")
        .replace("</code>", "`")
        .replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n")
        .replace("<p>", "\n\n")
        .replace("</p>", "")
}

/// Extract the description from a pre-parsed [`DocblockInfo`], applying
/// HTML-to-Markdown conversion.
pub(crate) fn extract_description_from_info(info: &DocblockInfo) -> Option<String> {
    info.description.as_deref().map(html_to_markdown)
}

/// Extract the human-readable description text from a raw docblock string.
///
/// Parses the docblock with `mago-docblock` and returns the free-text
/// content before the first `@tag`, with basic HTML converted to Markdown.
/// Returns `None` if no description text is present.
pub(crate) fn extract_docblock_description(docblock: Option<&str>) -> Option<String> {
    let raw = docblock?;
    let info = parse_docblock_for_tags(raw)?;
    extract_description_from_info(&info)
}

/// Shorten all namespace-qualified class names in a type string to their
/// short (unqualified) form.
///
/// Handles union (`|`), intersection (`&`), nullable (`?`), and generic
/// (`<`, `,`) type syntax.  For example:
///
///   - `"App\\Models\\User"` → `"User"`
///   - `"list<App\\Models\\User>"` → `"list<User>"`
///   - `"App\\Foo|App\\Bar|null"` → `"Foo|Bar|null"`
#[cfg(test)]
pub(crate) fn shorten_type_string(ty: &str) -> String {
    use crate::php_type::PhpType;

    let parsed = PhpType::parse(ty);
    if matches!(parsed, PhpType::Raw(_)) {
        // Fallback: if mago couldn't parse the type, apply
        // the old segment-based shortening so we still shorten
        // namespace-qualified names in unparseable type strings.
        return shorten_type_string_fallback(ty);
    }
    parsed.shorten().to_string()
}

/// Shorten a structured `PhpType` without a string round-trip.
///
/// This avoids the `PhpType → String → PhpType::parse → shorten → String`
/// cycle that `shorten_type_string` incurs when the caller already has a
/// `PhpType` value.
pub(crate) fn shorten_php_type(ty: &PhpType) -> String {
    // Defensive fallback: in practice `Raw` values never reach this function
    // because all callers pass `PhpType` values from struct fields
    // (`type_hint`, `return_type`, `native_return_type`) that are populated
    // via `PhpType::parse()`, which only produces `Raw` for completely
    // unparseable strings.  The guard remains so that future callers that
    // might pass `Raw` values still get reasonable short names instead of
    // fully-qualified namespace paths.
    if matches!(ty, PhpType::Raw(_)) {
        return shorten_type_string_fallback(&ty.to_string());
    }
    ty.shorten().to_string()
}

/// Fallback character-by-character shortener for type strings that
/// `mago_type_syntax` cannot parse.  Splits on delimiter characters
/// (`|`, `&`, `<`, `>`, `,`, etc.) and shortens each segment
/// individually.
fn shorten_type_string_fallback(ty: &str) -> String {
    let mut result = String::with_capacity(ty.len());
    let mut segment_start = 0;
    let bytes = ty.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        if matches!(
            b,
            b'|' | b'&' | b'<' | b'>' | b',' | b' ' | b'?' | b'{' | b'}' | b':' | b'(' | b')'
        ) {
            if i > segment_start {
                result.push_str(crate::util::short_name(&ty[segment_start..i]));
            }
            result.push(b as char);
            segment_start = i + 1;
        }
    }
    // Flush trailing segment.
    if segment_start < ty.len() {
        result.push_str(crate::util::short_name(&ty[segment_start..]));
    }
    result
}
