//! Completion of the methods and functions a Drupal form or render
//! callback names.
//!
//! A string in one of the keys Drupal reads callbacks from (see
//! [`crate::drupal::forms`]) completes what it can name so far: after
//! `'::`, a public method of the form it is written in, or of the trait;
//! after `'Class::`, a public method of that class, the name read as fully
//! qualified; after `'service:`, a public method of the service's class;
//! and before any colon, a function.  A class name joined to `'::` names
//! a method of that class.  The buffer is mid-edit and usually does not
//! parse, so the key is recognised from the text before the string, in
//! the shapes the symbol map reads.

use std::borrow::Cow;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::Fqn;
use crate::completion::laravel_string_keys::string_key_response;
use crate::completion::source::code_context::{CodeContext, OpenBracket};
use crate::completion::symfony_string_keys::OfferedMethods;
use crate::drupal::elements::{
    arrow_key, assignment_target, ends_with_keys, trailing_literal, value_key,
};
use crate::drupal::forms::{
    AJAX, AJAX_CALLBACK, CALLBACK_KEYS, CALLBACK_LIST_KEYS, LAZY_BUILDER, RENDER_CALLBACK_KEYS,
    VALUE_CALLBACK,
};
use crate::text_position::{offset_to_position, position_to_offset};
use crate::types::{ClassInfo, FileContext};

/// The callback string the cursor is typing in.
struct CallbackString<'a> {
    /// The key Drupal reads the callback under.
    key: &'a str,
    /// The class a concatenation joins the string to.
    joined: Option<JoinedClass<'a>>,
    /// Where the string's contents start.
    start: usize,
}

/// The class expression before `. '::method'`.
enum JoinedClass<'a> {
    /// `Name::class`, a name written in code.
    Named(&'a str),
    /// `static::class`, `self::class`, or `__CLASS__`.
    Own,
}

impl Backend {
    /// Complete the method or function a callback string names so far.
    pub(crate) fn try_drupal_callback_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
        ctx: &FileContext,
    ) -> Option<CompletionResponse> {
        let site = callback_string(content, code)?;
        let cursor = position_to_offset(content, position) as usize;
        let typed = content.get(site.start..cursor)?;
        // Drupal calls a value callback only when it is a native callable,
        // which a non-static method named by a string is not.
        let native = site.key == VALUE_CALLBACK;
        let offered = if native {
            OfferedMethods::Static
        } else {
            OfferedMethods::Any
        };
        let loader = self.resolved_class_loader(ctx);
        let own_class = || {
            crate::class_lookup::find_shared_class_at_offset(&ctx.classes, cursor as u32).cloned()
        };

        let (classes, method) = if let Some(joined) = site.joined {
            let method = typed.strip_prefix("::")?;
            let class = match joined {
                JoinedClass::Own => own_class()?,
                JoinedClass::Named(name) => loader(Fqn::new(
                    &super::array_callable::resolve_class_name_to_fqn(self, name, ctx),
                ))?,
            };
            (vec![class], method)
        } else if let Some((class, method)) = typed.rsplit_once("::") {
            let classes = if class.is_empty() {
                if native || RENDER_CALLBACK_KEYS.contains(&site.key) {
                    return None;
                }
                crate::drupal::forms::form_callback_classes(vec![own_class()?], &loader)
            } else {
                // A class name in a string is a runtime name, which is fully
                // qualified whether or not it says so.
                let class = unescaped(class);
                let class = class.strip_prefix('\\').unwrap_or(&class);
                if !is_qualified_name(class) {
                    return None;
                }
                vec![loader(Fqn::new(class))?]
            };
            (classes, method)
        } else if let Some((service, method)) = typed.split_once(':') {
            if native || !crate::drupal::routing::is_plain_name(service) {
                return None;
            }
            let classes: Vec<Arc<ClassInfo>> = self
                .symfony_service_classes(service)
                .iter()
                .filter_map(|class| loader(Fqn::new(class)))
                .collect();
            (classes, method)
        } else {
            return self.callback_function_response(uri, content, site.start, typed, position);
        };

        if !is_identifier_prefix(method) {
            return None;
        }
        let names = self.public_method_names(&classes, &loader, method, offered);
        string_key_response(
            names,
            CompletionItemKind::Method,
            content,
            cursor - method.len(),
            position,
        )
    }

    /// The functions whose name contains the `typed` part of a callback
    /// string that starts at `start`, each written as the fully qualified
    /// name PHP reads a function name in a string as.
    fn callback_function_response(
        &self,
        uri: &str,
        content: &str,
        start: usize,
        typed: &str,
        position: Position,
    ) -> Option<CompletionResponse> {
        if !typed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\'))
        {
            return None;
        }
        let name = typed.trim_start_matches('\\');
        let range = Range::new(
            offset_to_position(content, start + typed.len() - name.len()),
            position,
        );
        // A double-quoted string reads some backslashes as escapes (`\n`,
        // `\f`, …), so a namespace separator is written escaped there.
        let double_quoted = content.as_bytes().get(start.wrapping_sub(1)) == Some(&b'"');
        let (mut items, is_incomplete) =
            self.build_function_completions(&unescaped(name), true, None, &None, uri, position);
        for item in &mut items {
            let new_text = item
                .insert_text
                .take()
                .unwrap_or_else(|| item.label.clone());
            let new_text = if double_quoted {
                new_text.replace('\\', "\\\\")
            } else {
                new_text
            };
            item.text_edit = Some(CompletionItemTextEdit::TextEdit(TextEdit {
                range,
                new_text,
            }));
        }
        (!items.is_empty()).then_some(CompletionResponse::CompletionList(CompletionList {
            is_incomplete,
            items,
            apply_kind: None,
            item_defaults: None,
        }))
    }
}

/// The callback string the cursor is in, when the key it is written under
/// is one Drupal reads a callback from.
fn callback_string<'a>(content: &'a str, code: &CodeContext<'_>) -> Option<CallbackString<'a>> {
    let (quote, _) = code.open_string?;
    let before = &content[..code.code_before.len()];
    let (before, joined) = match before.strip_suffix('.') {
        Some(joined) => {
            let (before, class) = joined_class(joined.trim_end())?;
            (before, Some(class))
        }
        None => (before, None),
    };
    let key = callback_key(content, before, &code.open_brackets)?;
    Some(CallbackString {
        key,
        joined,
        start: quote + 1,
    })
}

/// The code before the class expression `before` ends with, and the class
/// it names.
fn joined_class(before: &str) -> Option<(&str, JoinedClass<'_>)> {
    if let Some(rest) = before.strip_suffix("__CLASS__") {
        return Some((rest.trim_end(), JoinedClass::Own));
    }
    let at = before.len().checked_sub("::class".len())?;
    if !before.get(at..)?.eq_ignore_ascii_case("::class") {
        return None;
    }
    let class = before[..at].trim_end();
    let name_start = class
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '\\'))
        .map_or(0, |at| at + 1);
    let (rest, name) = class.split_at(name_start);
    if name.is_empty() || rest.ends_with(['$', '>', ':']) {
        return None;
    }
    let joined = if name.eq_ignore_ascii_case("static") || name.eq_ignore_ascii_case("self") {
        JoinedClass::Own
    } else if name.eq_ignore_ascii_case("parent") {
        return None;
    } else {
        JoinedClass::Named(name)
    };
    Some((rest.trim_end(), joined))
}

/// The callback key a value that starts after `before` is written under:
/// `'#key' => …` or `$a['#key'] = …` for a key that holds one callback,
/// an item of the list under a key that holds a list (the first item for
/// `#lazy_builder`), `$a['#key'][] = …`, and the `callback` of an `#ajax`
/// array.
fn callback_key<'a>(
    content: &'a str,
    before: &'a str,
    brackets: &[OpenBracket],
) -> Option<&'a str> {
    match before.as_bytes().last()? {
        b'>' | b'=' => {
            if let Some(list) =
                assignment_target(before).and_then(|target| target.trim_end().strip_suffix("[]"))
            {
                let key = trailing_literal(list.trim_end().strip_suffix(']')?)?;
                return CALLBACK_LIST_KEYS.contains(&key).then_some(key);
            }
            if let Some(list) = keyed_list_entry(content, before, brackets) {
                return Some(list);
            }
            let key = value_key(before)?;
            if CALLBACK_KEYS.contains(&key) {
                return Some(key);
            }
            let ajax = key == AJAX_CALLBACK
                && if arrow_key(before).is_some() {
                    brackets.last().is_some_and(|array| {
                        array.byte == b'[' && value_key(&content[..array.code_before]) == Some(AJAX)
                    })
                } else {
                    assignment_target(before)
                        .is_some_and(|target| ends_with_keys(target, &[AJAX, AJAX_CALLBACK]))
                };
            ajax.then_some(key)
        }
        b'[' | b',' => {
            let list = brackets.last().filter(|list| list.byte == b'[')?;
            let key = value_key(&content[..list.code_before])?;
            (CALLBACK_LIST_KEYS.contains(&key) || (key == LAZY_BUILDER && list.commas == 0))
                .then_some(key)
        }
        _ => None,
    }
}

/// The list key whose keyed entry a value that starts after `before` is:
/// `'#submit' => ['name' => …]` or `$form['#entity_builders']['name'] = …`.
fn keyed_list_entry<'a>(
    content: &'a str,
    before: &'a str,
    brackets: &[OpenBracket],
) -> Option<&'a str> {
    let key = if arrow_key(before).is_some() {
        let list = brackets.last().filter(|list| list.byte == b'[')?;
        value_key(&content[..list.code_before])?
    } else {
        let entry = assignment_target(before)?.trim_end().strip_suffix(']')?;
        let name = trailing_literal(entry)?;
        let entry = entry.trim_end();
        let list = entry[..entry.len() - name.len() - 2]
            .trim_end()
            .strip_suffix('[')?;
        trailing_literal(list.trim_end().strip_suffix(']')?)?
    };
    CALLBACK_LIST_KEYS.contains(&key).then_some(key)
}

/// `text` with the escaped backslashes of a string literal read.
fn unescaped(text: &str) -> Cow<'_, str> {
    if text.contains("\\\\") {
        Cow::Owned(text.replace("\\\\", "\\"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Whether `name` is a class name, its namespace included.
fn is_qualified_name(name: &str) -> bool {
    name.split('\\').all(|part| {
        part.bytes()
            .next()
            .is_some_and(|first| first == b'_' || first.is_ascii_alphabetic())
            && part
                .bytes()
                .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
    })
}

/// Whether `name` can start a method name, or is empty.
fn is_identifier_prefix(name: &str) -> bool {
    name.bytes()
        .all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}
