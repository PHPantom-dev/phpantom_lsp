//! Rewriting a docblock to match its function: `@param` lines rebuilt from
//! the signature (keeping their descriptions), missing `@throws` added, and
//! `@return` corrected or enriched, with every other line kept as written.

use std::collections::HashMap;

use crate::class_lookup::{ResolvedClassLoader, SourceClassLoader};
use crate::completion::phpdoc::generation::{enrichment_plain, enrichment_plain_typed};
use crate::php_type::PhpType;

use super::changes::{is_type_contradiction, should_remove_return, should_update_return};
use super::{BodyFacts, DocParam, FunctionWithDocblock, SigParam};

/// The file's imports and class loader, for spelling the types a rewrite
/// writes.
struct TypeSpelling<'a> {
    class_loader: &'a SourceClassLoader<'a>,
    use_map: &'a HashMap<String, String>,
    file_namespace: &'a Option<String>,
}

impl TypeSpelling<'_> {
    /// The enriched docblock spelling of a native type hint (generic
    /// arguments, a callable signature), when it has one.
    fn enriched(&self, hint: Option<&PhpType>) -> Option<String> {
        enrichment_plain(hint, self.class_loader, self.use_map, self.file_namespace)
    }
}

/// Build the updated docblock text.
pub(super) fn build_updated_docblock(
    info: &FunctionWithDocblock,
    facts: &BodyFacts,
    class_loader: &SourceClassLoader<'_>,
    resolved_class_loader: &ResolvedClassLoader<'_>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let spelling = TypeSpelling {
        class_loader,
        use_map,
        file_namespace,
    };
    let mut lines = parse_docblock_lines(&info.docblock_text);
    remove_param_lines(&mut lines);

    // Remove @return if it's redundant (void) or contradicted.
    if should_remove_return(info) {
        lines.retain(|l| !matches!(l, DocLine::Return(_)));
    }

    let insert_pos = find_param_insert_position(&lines);
    for (i, param_line) in param_lines(info, &spelling).into_iter().enumerate() {
        lines.insert(insert_pos + i, param_line);
    }

    // Add missing @throws tags.
    if !facts.missing_throws.is_empty() {
        let throws_insert_pos = find_throws_insert_position(&lines);
        for (i, exc) in facts.missing_throws.iter().enumerate() {
            let exc = crate::names::shorten_class_fqn(exc, use_map, file_namespace);
            lines.insert(
                throws_insert_pos + i,
                DocLine::OtherTag(format!("@throws {}", exc)),
            );
        }
    }

    // Update @return type if needed.
    if should_update_return(info)
        && let Some(sig_ret) = &info.sig_return
    {
        set_return_type(&mut lines, info, &sig_ret.to_string());
    }

    // Body-based @return enrichment.
    if let Some(enriched) = &facts.enriched_return {
        let enriched = crate::names::shorten_type_for_file(
            enriched,
            use_map,
            file_namespace,
            resolved_class_loader,
        )
        .to_string();
        // Update the existing @return line or insert a new one.
        if !set_return_type(&mut lines, info, &enriched) {
            let close_pos = lines
                .iter()
                .position(|l| matches!(l, DocLine::Close))
                .unwrap_or(lines.len());
            lines.insert(close_pos, DocLine::Return(format!("@return {}", enriched)));
        }
    }

    rebuild_docblock(&lines, &info.indent)
}

/// Drop every `@param` line (they are rebuilt from the signature), and the
/// empty line that only separated them from the opening `/**`.
fn remove_param_lines(lines: &mut Vec<DocLine>) {
    lines.retain(|l| !matches!(l, DocLine::Param(_)));

    // Clean up orphaned empty lines left after removing @param lines.
    // Remove Empty lines that directly follow Open (no summary text).
    while lines.len() >= 2
        && matches!(lines[0], DocLine::Open)
        && matches!(lines[1], DocLine::Empty)
        && lines.get(2).is_some_and(|l| !matches!(l, DocLine::Text(_)))
    {
        lines.remove(1);
    }
}

/// The `@param` lines for the signature, in its order, with the types
/// aligned in one column and each tag's description kept.
fn param_lines(info: &FunctionWithDocblock, spelling: &TypeSpelling) -> Vec<DocLine> {
    // (type_str, name_with_prefix, description) per documented parameter.
    let entries: Vec<(String, String, String)> = info
        .sig_params
        .iter()
        .filter_map(|sig| {
            // Try to preserve the existing description for this param.
            let existing = info.doc_param(&sig.name);
            let type_str = param_doc_type(info, sig, existing, spelling)?;
            let description = existing.map(|e| e.description.clone()).unwrap_or_default();
            let name_prefix = if sig.is_variadic { "..." } else { "" };
            Some((
                type_str,
                format!("{}{}", name_prefix, sig.name),
                description,
            ))
        })
        .collect();

    // Compute max type width for column alignment.
    let max_type_len = entries.iter().map(|(t, _, _)| t.len()).max().unwrap_or(0);

    entries
        .iter()
        .map(|(type_str, name, description)| {
            let padding = " ".repeat(max_type_len - type_str.len());
            let line_text = if description.is_empty() {
                format!("@param {}{} {}", type_str, padding, name)
            } else {
                format!("@param {}{} {} {}", type_str, padding, name, description)
            };
            DocLine::Param(line_text)
        })
        .collect()
}

/// The type a signature parameter's `@param` tag should carry, or `None`
/// when the parameter gets no tag.
fn param_doc_type(
    info: &FunctionWithDocblock,
    sig: &SigParam,
    existing: Option<&DocParam>,
    spelling: &TypeSpelling,
) -> Option<String> {
    let Some(existing) = existing else {
        if info.doc_params.is_empty() {
            // No @param tags at all — only add a tag when the native
            // type needs enrichment, matching generate-docblock behaviour.
            return spelling.enriched(sig.type_hint.as_ref());
        }
        // The docblock already documents some params, so add this
        // missing one — use enrichment or fall back to raw hint / mixed.
        return Some(
            spelling
                .enriched(sig.type_hint.as_ref())
                .unwrap_or_else(|| {
                    sig.type_hint
                        .as_ref()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| PhpType::mixed().to_string())
                }),
        );
    };
    let Some(native) = &sig.type_hint else {
        return Some(existing.type_str_raw.clone());
    };
    if is_type_contradiction(&existing.type_parsed, native) {
        // Type is contradicted — try enrichment first, fall back to the
        // raw native hint.
        return Some(
            spelling
                .enriched(sig.type_hint.as_ref())
                .unwrap_or_else(|| native.to_string()),
        );
    }
    if existing.type_parsed.has_type_structure() {
        // Doc already has generics / callable / shape — keep it.
        return Some(existing.type_str_raw.clone());
    }
    // Check if enrichment would upgrade the type (e.g. bare `Closure` →
    // `(Closure(): mixed)`).
    let upgrades = enrichment_plain_typed(
        sig.type_hint.as_ref(),
        spelling.class_loader,
        spelling.use_map,
        spelling.file_namespace,
    )
    .is_some_and(|enriched| !enriched.equivalent(&existing.type_parsed));
    if upgrades {
        return Some(
            spelling
                .enriched(sig.type_hint.as_ref())
                .unwrap_or_else(|| existing.type_str_raw.clone()),
        );
    }
    Some(existing.type_str_raw.clone())
}

/// Give the first `@return` line the type `ty`, keeping the description
/// the docblock had.  Returns whether there was one.
fn set_return_type(lines: &mut [DocLine], info: &FunctionWithDocblock, ty: &str) -> bool {
    let Some(text) = lines.iter_mut().find_map(|line| match line {
        DocLine::Return(text) => Some(text),
        _ => None,
    }) else {
        return false;
    };
    let description = info
        .doc_return
        .as_ref()
        .map(|dr| dr.description.as_str())
        .unwrap_or("");
    *text = if description.is_empty() {
        format!("@return {}", ty)
    } else {
        format!("@return {} {}", ty, description)
    };
    true
}

/// Categorized docblock line.
#[derive(Debug, Clone)]
enum DocLine {
    /// Opening `/**`.
    Open,
    /// Closing `*/`.
    Close,
    /// A summary or description line (not a tag).
    Text(String),
    /// A `@param` tag line.
    Param(String),
    /// A `@return` tag line.
    Return(String),
    /// Any other tag line (`@throws`, `@template`, `@deprecated`, etc.).
    OtherTag(String),
    /// An empty line (just ` * `).
    Empty,
}

/// Parse a docblock into categorized lines.
fn parse_docblock_lines(docblock: &str) -> Vec<DocLine> {
    let mut result = Vec::new();
    let lines: Vec<&str> = docblock.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if i == 0 && trimmed.starts_with("/**") {
            // Single-line docblock: `/** @return void */`
            if trimmed.ends_with("*/") && trimmed.len() > 5 {
                let inner = trimmed
                    .strip_prefix("/**")
                    .unwrap_or("")
                    .strip_suffix("*/")
                    .unwrap_or("")
                    .trim();
                result.push(DocLine::Open);
                if !inner.is_empty() {
                    categorize_tag_line(inner, &mut result);
                }
                result.push(DocLine::Close);
                continue;
            }
            result.push(DocLine::Open);
            // Check if there's content after `/**` on the same line.
            let after_open = trimmed.strip_prefix("/**").unwrap_or("").trim();
            if !after_open.is_empty() {
                categorize_tag_line(after_open, &mut result);
            }
            continue;
        }

        if trimmed == "*/" || trimmed.ends_with("*/") {
            // Check if there's content before `*/`.
            let before_close = trimmed.strip_suffix("*/").unwrap_or("").trim();
            let before_close = before_close
                .strip_prefix('*')
                .unwrap_or(before_close)
                .trim();
            if !before_close.is_empty() {
                categorize_tag_line(before_close, &mut result);
            }
            result.push(DocLine::Close);
            continue;
        }

        // Regular docblock line: ` * content`
        let content = trimmed.strip_prefix('*').unwrap_or(trimmed).trim();

        // Check if this is a continuation line (no `@` prefix, preceded by
        // a tag line). If so, merge it into the previous tag line.
        if !content.is_empty()
            && !content.starts_with('@')
            && !result.is_empty()
            && matches!(
                result.last(),
                Some(DocLine::Param(_)) | Some(DocLine::Return(_)) | Some(DocLine::OtherTag(_))
            )
        {
            match result.last_mut() {
                Some(DocLine::Param(text))
                | Some(DocLine::Return(text))
                | Some(DocLine::OtherTag(text)) => {
                    text.push(' ');
                    text.push_str(content);
                }
                _ => {}
            }
            continue;
        }

        if content.is_empty() {
            result.push(DocLine::Empty);
        } else {
            categorize_tag_line(content, &mut result);
        }
    }

    result
}

/// Categorize a single content line (without the `*` prefix).
fn categorize_tag_line(content: &str, result: &mut Vec<DocLine>) {
    if content.starts_with("@param") {
        result.push(DocLine::Param(content.to_string()));
    } else if content.starts_with("@return") {
        result.push(DocLine::Return(content.to_string()));
    } else if content.starts_with('@') {
        result.push(DocLine::OtherTag(content.to_string()));
    } else {
        result.push(DocLine::Text(content.to_string()));
    }
}

/// Find the position to insert new `@param` lines.
fn find_param_insert_position(lines: &[DocLine]) -> usize {
    // Insert before the first @return, @throws, or other tag that comes
    // after any text/summary.
    let mut last_text_or_empty = None;
    let mut first_return_or_throws = None;

    for (i, line) in lines.iter().enumerate() {
        match line {
            DocLine::Text(_) | DocLine::Empty => {
                last_text_or_empty = Some(i);
            }
            DocLine::Return(_) if first_return_or_throws.is_none() => {
                first_return_or_throws = Some(i);
            }
            DocLine::OtherTag(text)
                if (text.starts_with("@throws") || text.starts_with("@return"))
                    && first_return_or_throws.is_none() =>
            {
                first_return_or_throws = Some(i);
            }
            _ => {}
        }
    }

    // Prefer inserting before @return/@throws.
    if let Some(pos) = first_return_or_throws {
        return pos;
    }

    // Otherwise insert after the last text/empty line.
    if let Some(pos) = last_text_or_empty {
        return pos + 1;
    }

    // Fallback: insert before Close.
    for (i, line) in lines.iter().enumerate() {
        if matches!(line, DocLine::Close) {
            return i;
        }
    }

    lines.len()
}

/// Find the position to insert new `@throws` lines.
fn find_throws_insert_position(lines: &[DocLine]) -> usize {
    // Insert after the last existing @throws tag.
    let mut last_throws = None;
    let mut first_return = None;

    for (i, line) in lines.iter().enumerate() {
        match line {
            DocLine::OtherTag(text) if text.starts_with("@throws") => {
                last_throws = Some(i);
            }
            DocLine::Return(_) if first_return.is_none() => {
                first_return = Some(i);
            }
            _ => {}
        }
    }

    // After the last existing @throws.
    if let Some(pos) = last_throws {
        return pos + 1;
    }

    // Before @return (but after any blank separator preceding it).
    if let Some(pos) = first_return {
        // If the line before @return is Empty, insert before that too.
        if pos > 0 && matches!(lines.get(pos - 1), Some(DocLine::Empty)) {
            return pos - 1;
        }
        return pos;
    }

    // After the last @param.
    let mut last_param = None;
    for (i, line) in lines.iter().enumerate() {
        if matches!(line, DocLine::Param(_)) {
            last_param = Some(i);
        }
    }
    if let Some(pos) = last_param {
        return pos + 1;
    }

    // Fallback: before Close.
    for (i, line) in lines.iter().enumerate() {
        if matches!(line, DocLine::Close) {
            return i;
        }
    }

    lines.len()
}

/// Rebuild a docblock string from categorized lines.
fn rebuild_docblock(lines: &[DocLine], indent: &str) -> String {
    let mut result = String::new();
    let mut prev_was_param = false;
    let mut prev_was_text_or_empty = false;

    for (i, line) in lines.iter().enumerate() {
        match line {
            DocLine::Open => {
                result.push_str("/**");
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
            DocLine::Close => {
                result.push_str(indent);
                result.push_str(" */");
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
            DocLine::Text(text) => {
                // Add blank separator before text if preceded by tags.
                if prev_was_param {
                    result.push_str(indent);
                    result.push_str(" *\n");
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = true;
            }
            DocLine::Empty => {
                result.push_str(indent);
                result.push_str(" *\n");
                prev_was_param = false;
                prev_was_text_or_empty = true;
            }
            DocLine::Param(text) => {
                // Add blank separator before first @param if preceded by text.
                if !prev_was_param && prev_was_text_or_empty {
                    // Check if the previous line was already empty.
                    let prev_empty = i > 0 && matches!(lines.get(i - 1), Some(DocLine::Empty));
                    if !prev_empty {
                        result.push_str(indent);
                        result.push_str(" *\n");
                    }
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = true;
                prev_was_text_or_empty = false;
            }
            DocLine::Return(text) => {
                // Add blank separator before @return if preceded by @param.
                if prev_was_param {
                    result.push_str(indent);
                    result.push_str(" *\n");
                }
                // Add blank separator if preceded by text without a blank line.
                if prev_was_text_or_empty && !prev_was_param {
                    let prev_empty = i > 0 && matches!(lines.get(i - 1), Some(DocLine::Empty));
                    if !prev_empty {
                        result.push_str(indent);
                        result.push_str(" *\n");
                    }
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
            DocLine::OtherTag(text) => {
                if prev_was_param {
                    result.push_str(indent);
                    result.push_str(" *\n");
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
        }
    }

    result
}
