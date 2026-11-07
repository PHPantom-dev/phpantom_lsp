//! Completion for the keys of a Yii DI definition.
//!
//! A key of a definition array names a method to call (`'setLogger()'`, or
//! `'__construct()'` for the constructor's arguments) or a property to set
//! (`'$pageSize'`) on the class the definition builds, and a key of the
//! array a method key is given names one of the method's parameters.  The
//! class is the one the array's own `class` entry names, or the one it is
//! keyed by (`MailerInterface::class => ['|' …`), as extraction reads it
//! (see `symbol_map/extraction/yii_definitions.rs`).

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::{CodeContext, OpenBracket};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::{FileContext, Visibility};
use crate::yii::definitions::definition_key;

impl Backend {
    /// Complete a key of a DI definition, or of the arguments one of its
    /// method keys passes.  `None` when the cursor is on neither.
    pub(crate) fn try_yii_definition_completion(
        &self,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let (quote, quote_char) = code.open_string?;
        let cursor = position_to_offset(content, position) as usize;
        let prefix = content.get(quote + 1..cursor)?;
        if !matches!(code.last_code_byte(), Some(b'[' | b',')) {
            return None;
        }
        let (array, outer) = match &code.open_brackets[..] {
            [.., outer, inner] => (inner, Some(outer)),
            [inner] => (inner, None),
            [] => return None,
        };
        if array.byte != b'[' {
            return None;
        }
        let rest = literal_end(content, cursor, quote_char);
        let edit_range = Range {
            start: offset_to_position(content, quote + 1),
            end: position,
        };

        // The arguments of a method key: `'__construct()' => ['|' …`.
        if let Some(outer) = outer.filter(|outer| outer.byte == b'[')
            && let Some(key) = crate::drupal::elements::arrow_key(&content[..array.code_before])
            && let Some((method, true)) = definition_key(key)
        {
            let fqn = self.definition_class(content, outer, quote, rest, ctx)?;
            let class = self.yii_defined_class(&fqn)?;
            let method = class.get_method_ci(method)?;
            let items: Vec<CompletionItem> = method
                .parameters
                .iter()
                .filter(|parameter| !parameter.is_variadic)
                .map(|parameter| {
                    let name = parameter
                        .name
                        .strip_prefix('$')
                        .unwrap_or(&parameter.name)
                        .to_string();
                    let detail = parameter.type_hint.as_ref().map(|ty| ty.to_string());
                    (name, detail)
                })
                .filter(|(name, _)| starts_with_ignoring_case(name, prefix))
                .enumerate()
                .map(|(at, (name, detail))| CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::Variable),
                    detail,
                    sort_text: Some(format!("{at:05}")),
                    filter_text: Some(name.clone()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range: edit_range,
                        new_text: name,
                    })),
                    ..Default::default()
                })
                .collect();
            return (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items));
        }

        // The keys of the definition itself.
        let fqn = self.definition_class(content, array, quote, rest, ctx)?;
        let class = self.yii_defined_class(&fqn)?;
        let short = crate::util::short_name(&fqn);
        let mut entries: Vec<(String, CompletionItemKind, String)> = Vec::new();
        if let Some(constructor) = class.get_method_ci("__construct") {
            entries.push((
                "__construct()".to_string(),
                CompletionItemKind::Constructor,
                format!("{short}::{}()", constructor.name),
            ));
        }
        for method in class.methods.iter() {
            if method.visibility != Visibility::Public || method.name.starts_with("__") {
                continue;
            }
            entries.push((
                format!("{}()", method.name),
                CompletionItemKind::Method,
                format!("{short}::{}()", method.name),
            ));
        }
        for property in class.properties.iter() {
            if property.visibility != Visibility::Public
                || property.is_static
                || property.is_readonly
            {
                continue;
            }
            entries.push((
                format!("${}", property.name),
                CompletionItemKind::Property,
                format!("{short}::${}", property.name),
            ));
        }
        let items: Vec<CompletionItem> = entries
            .into_iter()
            .filter(|(label, _, _)| starts_with_ignoring_case(label, prefix))
            .enumerate()
            .map(|(at, (label, kind, detail))| CompletionItem {
                label: label.clone(),
                kind: Some(kind),
                detail: Some(detail),
                sort_text: Some(format!("{at:05}")),
                filter_text: Some(label.clone()),
                text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: edit_range,
                    new_text: label,
                })),
                ..Default::default()
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }

    /// The class the definition array opened by `array` builds: the one its
    /// own `class` entry names, before or after the literal being typed
    /// (which starts at `quote` and is followed by `rest`), or the one the
    /// array is keyed by.
    fn definition_class(
        &self,
        content: &str,
        array: &OpenBracket,
        quote: usize,
        rest: usize,
        ctx: &FileContext,
    ) -> Option<String> {
        let (name, offset) = class_entry(content, array.offset + 1, quote)
            .or_else(|| class_entry(content, rest, content.len()))
            .or_else(|| keyed_by(&content[..array.code_before]))?;
        let fqn = match name.to_ascii_lowercase().as_str() {
            "self" | "static" => crate::class_lookup::find_class_at_offset(&ctx.classes, offset)?
                .fqn()
                .to_string(),
            _ => ctx.resolve_name_at(name, offset),
        };
        Some(fqn.trim_start_matches('\\').to_string())
    }
}

fn starts_with_ignoring_case(name: &str, prefix: &str) -> bool {
    name.as_bytes()
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Where the literal the cursor is in ends: past its closing quote on the
/// cursor's line, or at the end of the line when it is not closed yet.
fn literal_end(content: &str, cursor: usize, quote: char) -> usize {
    let bytes = content.as_bytes();
    let mut at = cursor;
    while at < bytes.len() && bytes[at] != b'\n' {
        if bytes[at] == b'\\' {
            at += 2;
            continue;
        }
        if bytes[at] == quote as u8 {
            return at + 1;
        }
        at += 1;
    }
    at.min(bytes.len())
}

/// The class a `Name::class` that `before` ends with, followed by `=>`,
/// names, with the offset of the name.
fn keyed_by(before: &str) -> Option<(&str, u32)> {
    let before = before.trim_end().strip_suffix("=>")?.trim_end();
    class_fetch_before(before)
}

/// The class name of the `Name::class` `text` ends with, and its offset.
fn class_fetch_before(text: &str) -> Option<(&str, u32)> {
    let cut = text.len().checked_sub("::class".len())?;
    if !text.get(cut..)?.eq_ignore_ascii_case("::class") {
        return None;
    }
    let name_end = cut;
    let name_start = text[..name_end]
        .bytes()
        .rposition(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b >= 0x80))
        .map_or(0, |at| at + 1);
    let name = &text[name_start..name_end];
    (!name.is_empty()).then_some((name, name_start as u32))
}

/// The class a `'class' => Name::class` entry directly inside the array
/// whose elements run from `from` names, looking no further than `until`
/// or the array's end.
fn class_entry(content: &str, from: usize, until: usize) -> Option<(&str, u32)> {
    let bytes = content.as_bytes();
    let until = until.min(bytes.len());
    let line_end =
        |from: usize| memchr::memchr(b'\n', &bytes[from..]).map_or(bytes.len(), |end| from + end);
    let mut depth = 0usize;
    let mut at = from;
    while at < until {
        match bytes[at] {
            quote @ (b'\'' | b'"') => {
                let mut end = at + 1;
                while end < bytes.len() && bytes[end] != quote {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                if depth == 0
                    && &content[at + 1..end.min(bytes.len())] == "class"
                    && let Some(found) = class_entry_value(content, end + 1)
                {
                    return Some(found);
                }
                at = end + 1;
                continue;
            }
            b'/' if bytes.get(at + 1) == Some(&b'*') => {
                at = memchr::memmem::find(&bytes[at + 2..], b"*/")
                    .map_or(bytes.len(), |end| at + 2 + end + 2);
                continue;
            }
            b'/' if bytes.get(at + 1) == Some(&b'/') => {
                at = line_end(at);
                continue;
            }
            b'#' if bytes.get(at + 1) != Some(&b'[') => {
                at = line_end(at);
                continue;
            }
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            b';' if depth == 0 => return None,
            _ => {}
        }
        at += 1;
    }
    None
}

/// The class of the `=> Name::class` that follows a `'class'` key ending
/// just before `from`.
fn class_entry_value(content: &str, from: usize) -> Option<(&str, u32)> {
    let rest = content.get(from..)?;
    let after_arrow = rest.trim_start().strip_prefix("=>")?;
    let value = after_arrow.trim_start();
    let value_start = content.len() - value.len();
    let name_len = value
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b >= 0x80))
        .unwrap_or(value.len());
    let name = &value[..name_len];
    let fetch = value[name_len..].get(.."::class".len())?;
    (!name.is_empty() && fetch.eq_ignore_ascii_case("::class"))
        .then_some((name, value_start as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_class_entry_names_the_class() {
        let php = "['__construct()' => [], 'class' => Mailer::class, '' => 1]";
        let (name, offset) = class_entry(php, 1, php.len()).unwrap();
        assert_eq!(name, "Mailer");
        assert_eq!(&php[offset as usize..offset as usize + 6], "Mailer");
        // A `class` entry of a nested array is that array's.
        let nested = "['x' => ['class' => Other::class], '$a' => 1]";
        assert_eq!(class_entry(nested, 1, nested.len()), None);
        assert_eq!(class_entry("['class' => 'btn']", 1, 18), None);
    }

    #[test]
    fn a_definition_is_keyed_by_its_class() {
        let before = "return [\n    \\App\\MailerInterface::class =>";
        let (name, offset) = keyed_by(before).unwrap();
        assert_eq!(name, "\\App\\MailerInterface");
        assert_eq!(&before[offset as usize..offset as usize + 4], "\\App");
        assert_eq!(keyed_by("return ['mailer' =>"), None);
        assert_eq!(
            keyed_by("return [self::class =>").map(|(n, _)| n),
            Some("self")
        );
    }
}
