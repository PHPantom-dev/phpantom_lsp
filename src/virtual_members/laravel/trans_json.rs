//! JSON translation declarations with their source positions.

use tower_lsp::lsp_types::{Location, Position, Url};

use crate::Backend;
use crate::symbol_map::LaravelStringKind;
use crate::text_position::position_to_offset;

use super::trans_keys::TransKeyMatch;

/// Parse a flat JSON translation object while retaining the byte range of
/// each key's source spelling, including escapes. Invalid documents yield
/// no declarations; values are decoded by serde rather than a text scanner.
pub(super) fn collect_json_trans_declarations(content: &str) -> Vec<TransKeyMatch> {
    parse_declarations(content).unwrap_or_default()
}

fn parse_declarations(content: &str) -> Option<Vec<TransKeyMatch>> {
    let mut input = content
        .trim_start_matches([' ', '\t', '\r', '\n'])
        .strip_prefix('{')?
        .trim_start_matches([' ', '\t', '\r', '\n']);
    let mut out = Vec::new();
    if let Some(rest) = input.strip_prefix('}') {
        return rest
            .trim_matches([' ', '\t', '\r', '\n'])
            .is_empty()
            .then_some(out);
    }
    loop {
        let start = content.len() - input.len() + 1;
        let mut keys = serde_json::Deserializer::from_str(input).into_iter::<String>();
        let key = keys.next()?.ok()?;
        let consumed = keys.byte_offset();
        let end = start + consumed - 2;
        input = input[consumed..]
            .trim_start_matches([' ', '\t', '\r', '\n'])
            .strip_prefix(':')?
            .trim_start_matches([' ', '\t', '\r', '\n']);
        let mut values = serde_json::Deserializer::from_str(input).into_iter::<serde_json::Value>();
        let value = values.next()?.ok()?;
        input = input[values.byte_offset()..].trim_start_matches([' ', '\t', '\r', '\n']);
        out.push(TransKeyMatch {
            key,
            start,
            end,
            is_group: false,
            value: value.as_str().map(str::to_string),
        });
        if let Some(rest) = input.strip_prefix('}') {
            return rest
                .trim_matches([' ', '\t', '\r', '\n'])
                .is_empty()
                .then_some(out);
        }
        input = input
            .strip_prefix(',')?
            .trim_start_matches([' ', '\t', '\r', '\n']);
    }
}

/// Find uses of the JSON translation key under the cursor through the
/// same PHP/Blade reference index as a translation helper call.
pub(crate) fn find_json_trans_references(
    backend: &Backend,
    uri: &str,
    content: &str,
    position: Position,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    if !uri.ends_with(".json") {
        return None;
    }
    let path = Url::parse(uri).ok()?.to_file_path().ok()?;
    let parent = path.parent()?;
    if !parent.ends_with("lang")
        && !backend
            .laravel_provider_resources
            .read()
            .trans_dirs
            .iter()
            .any(|dir| dir.namespace.is_empty() && dir.path == parent)
    {
        return None;
    }
    let offset = position_to_offset(content, position) as usize;
    let declaration = collect_json_trans_declarations(content)
        .into_iter()
        .find(|declaration| declaration.start <= offset && offset <= declaration.end)?;
    let kind = LaravelStringKind::Trans;
    let snapshot = backend.user_file_symbol_maps_for_reference_keys(&[
        crate::reference_index::ReferenceIndexKey::LaravelString {
            kind: kind.clone(),
            key: declaration.key.clone(),
        },
    ]);
    Some(super::string_keys::find_laravel_string_key_references(
        backend,
        &kind,
        &declaration.key,
        uri,
        &snapshot,
        include_declaration,
    ))
}

#[cfg(test)]
#[path = "trans_json_tests.rs"]
mod tests;
