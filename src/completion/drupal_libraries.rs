//! Completion of the Drupal libraries a render array attaches.
//!
//! The strings of the `library` list under a render array's `#attached`
//! are library names, which the libraries files declare (see
//! [`crate::drupal::libraries`]).  The buffer is mid-edit and usually does
//! not parse, so the list is recognised from the text before the cursor,
//! in the shapes the symbol map reads: `'#attached' => ['library' => ['|`,
//! `$build['#attached']['library'][] = '|`, and assignments of a whole
//! `library` list or `#attached` array.

use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::drupal::elements::{arrow_key, assignment_target};
use crate::symbol_map::StringKeyKind;
use crate::text_position::position_to_offset;

const ATTACHED: &str = "#attached";
const LIBRARY: &str = "library";

impl Backend {
    /// Complete a library name in the `library` list of a render array's
    /// `#attached`.
    pub(crate) fn try_drupal_library_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let start = attached_library_start(content, code)?;
        let cursor = position_to_offset(content, position) as usize;
        self.symfony_key_response(
            uri,
            StringKeyKind::DrupalLibrary,
            content,
            start,
            cursor,
            position,
        )
    }
}

/// Where the library name the cursor is typing starts, when the string it
/// is in is an item of the `library` list of an `#attached`.
fn attached_library_start(content: &str, code: &CodeContext<'_>) -> Option<usize> {
    let (quote, _) = code.open_string?;
    let reads_library = match code.last_code_byte()? {
        // `$build['#attached']['library'][] = '|`
        b'=' => assignment_target(code.code_before)
            .and_then(|target| target.trim_end().strip_suffix("[]"))
            .is_some_and(|target| ends_with_keys(target, &[ATTACHED, LIBRARY])),
        b'[' | b',' => {
            let (list, enclosing) = match &code.open_brackets[..] {
                [.., enclosing, list] => (list, Some(enclosing)),
                [list] => (list, None),
                [] => return None,
            };
            let before_list = &content[..list.code_before];
            if list.byte != b'[' {
                false
            } else if arrow_key(before_list) == Some(LIBRARY) {
                // `'#attached' => ['library' => ['|` or
                // `$build['#attached'] = ['library' => ['|`
                enclosing.is_some_and(|attached| {
                    let before = &content[..attached.code_before];
                    attached.byte == b'['
                        && (arrow_key(before) == Some(ATTACHED)
                            || assignment_target(before)
                                .is_some_and(|target| ends_with_keys(target, &[ATTACHED])))
                })
            } else {
                // `$build['#attached']['library'] = ['|`
                assignment_target(before_list)
                    .is_some_and(|target| ends_with_keys(target, &[ATTACHED, LIBRARY]))
            }
        }
        _ => false,
    };
    reads_library.then_some(quote + 1)
}

/// Whether `target` ends with the offsets `['keys…']`, in order.
fn ends_with_keys(mut target: &str, keys: &[&str]) -> bool {
    for key in keys.iter().rev() {
        let Some(rest) = target
            .trim_end()
            .strip_suffix(']')
            .and_then(|rest| strip_quoted(rest.trim_end(), key))
            .and_then(|rest| rest.trim_end().strip_suffix('['))
        else {
            return false;
        };
        target = rest;
    }
    true
}

/// `text` without the string literal `key` it ends with.
fn strip_quoted<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    ['\'', '"'].into_iter().find_map(|quote| {
        text.strip_suffix(quote)?
            .strip_suffix(key)?
            .strip_suffix(quote)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::source::code_context::code_context_at;

    /// Where the name starts, for a cursor at the end of `php`.
    fn start(php: &str) -> Option<usize> {
        let code = code_context_at(php, php.len())?;
        attached_library_start(php, &code)
    }

    #[test]
    fn the_library_list_of_attached_reads_library_names() {
        for php in [
            "<?php\nreturn ['#attached' => ['library' => ['co",
            "<?php\nreturn [\n  '#attached' => [\n    'library' => [\n      'acme/one',\n      \"co",
            "<?php\n$build['#attached']['library'][] = 'co",
            "<?php\n$build['actions'][\"#attached\"]['library'] [] = 'co",
            "<?php\n$build['#attached']['library'] = ['acme/one', 'co",
            "<?php\n$build['#attached'] = ['library' => ['co",
        ] {
            assert_eq!(start(php), Some(php.len() - 2), "{php}");
        }
    }

    #[test]
    fn other_strings_read_nothing() {
        for php in [
            "<?php\nreturn ['library' => ['co",
            "<?php\nreturn ['#attached' => ['drupalSettings' => ['co",
            "<?php\nreturn ['#attached' => ['library' => 'co",
            "<?php\nreturn ['#attached' => ['co",
            "<?php\n$build['library'][] = 'co",
            "<?php\n$build['#attached']['library'][] .= 'co",
            "<?php\nif ($build['#attached']['library'] == 'co",
            "<?php\n$build['#attached']['library'] = 'co",
            "<?php\nf(['#attached' => ['library' => [], 'co",
        ] {
            assert_eq!(start(php), None, "{php}");
        }
    }
}
