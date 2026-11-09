//! Yii params key completion: the keys the merged params declare under the
//! ones a `$params` read names before the string being typed, offered in a
//! configuration file `yiisoft/config` hands the `$params`.

use tower_lsp_server::gen_lsp_types::{CompletionResponse, Position};

use crate::Backend;
use crate::completion::laravel_string_keys::{string_key_item_kind, string_key_response};
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::StringKeyKind;
use crate::text_position::position_to_offset;

impl Backend {
    /// Complete the key of a `$params['…']['|']` read.
    pub(crate) fn try_yii_param_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let keys = params_read_before(content, quote)?;
        let path = crate::util::uri_to_path(uri)?;
        if !self.yii_receives_params(&path) {
            return None;
        }
        let params = self.yii_params();
        let mut node = params.root();
        for key in &keys {
            node = node.child(key)?;
        }
        let names = node.child_names().map(str::to_string).collect();
        let cursor = position_to_offset(content, position) as usize;
        if content.get(quote + 1..cursor)?.contains(['\'', '"']) {
            return None;
        }
        string_key_response(
            names,
            string_key_item_kind(&StringKeyKind::YiiParam),
            content,
            quote + 1,
            position,
        )
    }
}

/// The keys a `$params` read names before the string whose opening quote is
/// at `quote`, when that string is the next key of one:
/// `$params['application'][` gives `["application"]`, and `$params[` none.
fn params_read_before(content: &str, quote: usize) -> Option<Vec<&str>> {
    let mut before = content.get(..quote)?.trim_end().strip_suffix('[')?;
    let mut keys = Vec::new();
    loop {
        before = before.trim_end();
        if let Some(rest) = before.strip_suffix("$params") {
            let continues = rest
                .as_bytes()
                .last()
                .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80);
            if continues {
                return None;
            }
            keys.reverse();
            return Some(keys);
        }
        // `['key']`, read from its end.
        let rest = before.strip_suffix(']')?.trim_end();
        let close = *rest.as_bytes().last()?;
        if close != b'\'' && close != b'"' {
            return None;
        }
        let rest = &rest[..rest.len() - 1];
        let open = rest.rfind(close as char)?;
        let key = &rest[open + 1..];
        if key.contains('\\') || (close == b'"' && key.contains('$')) {
            return None;
        }
        keys.push(key);
        before = rest[..open].trim_end().strip_suffix('[')?;
    }
}

#[cfg(test)]
mod tests {
    use super::params_read_before;

    #[test]
    fn the_keys_before_the_string_are_read() {
        fn read(php: &str) -> Option<Vec<&str>> {
            params_read_before(php, php.rfind('\'').unwrap())
        }
        assert_eq!(read("$params['"), Some(vec![]));
        assert_eq!(read("$params['a']['"), Some(vec!["a"]));
        assert_eq!(
            read("$params[ 'yiisoft/view' ][\"parameters\"] ['"),
            Some(vec!["yiisoft/view", "parameters"])
        );
        assert_eq!(read("$otherParams['"), None);
        assert_eq!(read("$config['"), None);
        assert_eq!(read("f('"), None);
        assert_eq!(read("$params[$key]['"), None);
    }
}
