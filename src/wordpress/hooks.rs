//! WordPress hook names: the string that ties a listener to the place that
//! fires it.
//!
//! `do_action('acme_order_placed', $order)` fires a hook and
//! `add_action('acme_order_placed', …)` listens to it. Each is a
//! [`StringKey`](crate::symbol_map::SymbolKind::StringKey) span of kind
//! [`WordPressHook`](StringKeyKind::WordPressHook): a firing is a
//! declaration (`is_write`), anything else a use.
//!
//! Core fires many hooks under a name it builds at runtime
//! (`"save_post_{$post->post_type}"`). Such a name is stored as a pattern:
//! its literal parts as written, and each part computed at runtime as its
//! source text in braces, which is how WordPress's own documentation names
//! the hook. A literal name matches a pattern when it holds the literal
//! parts in order, each computed part standing for at least one character,
//! and two patterns match when their literal parts are the same. A pattern's
//! span covers its first literal part with a word in it only, since the
//! computed parts carry spans of their own.
//!
//! The files that hold a hook span are tracked here, so completion and
//! navigation read those symbol maps rather than every file's. The hooks a
//! dependency fires (see [`super::hook_sources`]) complete and navigate
//! too, but are not the project's own: a rename does not reach them, and
//! refuses a name one of them fires.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tower_lsp_server::gen_lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemTextEdit, CompletionResponse,
    DocumentHighlight, DocumentHighlightKind, InsertTextFormat, Location, Position, Range,
    TextEdit,
};

use super::hook_sources::{DependencyHooks, listed_core_hooks};
use crate::Backend;
use crate::completion::laravel_string_keys::{ArgumentSlot, scalar_argument_at};
use crate::completion::source::code_context::CodeContext;
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolMap};
use crate::text_position::{LineIndex, offset_to_position, position_to_offset};

/// What a call to a hook function does with the hook it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookRole {
    /// It fires the hook, which declares it.
    Fires,
    /// It listens to the hook, or asks about it.
    Uses,
}

/// The functions that fire a hook named by their first argument.
const FIRING_FUNCTIONS: &[&str] = &[
    "do_action",
    "do_action_ref_array",
    "do_action_deprecated",
    "apply_filters",
    "apply_filters_ref_array",
    "apply_filters_deprecated",
];

/// The functions that listen to a hook named by their first argument, or
/// ask about it.
const USING_FUNCTIONS: &[&str] = &[
    "add_action",
    "add_filter",
    "remove_action",
    "remove_filter",
    "remove_all_actions",
    "remove_all_filters",
    "has_action",
    "has_filter",
    "did_action",
    "did_filter",
    "doing_action",
    "doing_filter",
];

/// The name every hook function gives the parameter that takes the hook.
pub(crate) const HOOK_PARAMETER: &str = "hook_name";

/// What the global function `name` does with the hook its first argument
/// names, when it is one of WordPress's hook functions.
pub(crate) fn hook_function_role(name: &str) -> Option<HookRole> {
    let is_one_of = |names: &[&str]| names.iter().any(|n| name.eq_ignore_ascii_case(n));
    if is_one_of(FIRING_FUNCTIONS) {
        Some(HookRole::Fires)
    } else if is_one_of(USING_FUNCTIONS) {
        Some(HookRole::Uses)
    } else {
        None
    }
}

/// Whether the hook name `key` is a pattern, with a part computed at
/// runtime.
pub(crate) fn is_pattern(key: &str) -> bool {
    key.contains('{')
}

/// A pattern's literal parts and its computed parts' source text, in
/// order: one more literal part than computed ones, each possibly empty.
fn pattern_parts(pattern: &str) -> (Vec<&str>, Vec<&str>) {
    let mut literals = Vec::new();
    let mut computed = Vec::new();
    let mut literal_start = 0;
    let mut depth = 0usize;
    let mut computed_start = 0;
    for (index, byte) in pattern.bytes().enumerate() {
        match byte {
            b'{' => {
                if depth == 0 {
                    literals.push(&pattern[literal_start..index]);
                    computed_start = index + 1;
                }
                depth += 1;
            }
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    computed.push(&pattern[computed_start..index]);
                    literal_start = index + 1;
                }
            }
            _ => {}
        }
    }
    literals.push(&pattern[literal_start..]);
    (literals, computed)
}

/// Whether the literal hook name `name` is one the pattern `pattern` can
/// produce.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    let (literals, _) = pattern_parts(pattern);
    let Some((first, rest)) = literals.split_first() else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return pattern == name;
    };
    let Some(mut remaining) = name.strip_prefix(first) else {
        return false;
    };
    // Each computed part stands for at least one character, and placing
    // every literal part as early as it fits leaves the most room for the
    // ones after it.
    for literal in middle {
        let Some(skip) = remaining.chars().next().map(char::len_utf8) else {
            return false;
        };
        let Some(found) = remaining[skip..].find(literal) else {
            return false;
        };
        remaining = &remaining[skip + found + literal.len()..];
    }
    remaining.len() > last.len() && remaining.ends_with(last)
}

/// Whether two hook names, either of which may be a pattern, name the same
/// hook.
pub(crate) fn hook_names_match(a: &str, b: &str) -> bool {
    match (is_pattern(a), is_pattern(b)) {
        (false, false) => a == b,
        (true, false) => pattern_matches(a, b),
        (false, true) => pattern_matches(b, a),
        (true, true) => pattern_parts(a).0 == pattern_parts(b).0,
    }
}

/// The snippet completion inserts for the pattern `pattern`: its literal
/// parts, with a placeholder for each computed part, named after the last
/// name in its source text (`post_type` for `$post->post_type`).
fn pattern_snippet(pattern: &str) -> String {
    let (literals, computed) = pattern_parts(pattern);
    let mut snippet = String::with_capacity(pattern.len() + 4 * computed.len());
    for (index, literal) in literals.iter().enumerate() {
        for c in literal.chars() {
            if matches!(c, '$' | '}' | '\\') {
                snippet.push('\\');
            }
            snippet.push(c);
        }
        if let Some(source) = computed.get(index) {
            let trimmed = source.trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '_'));
            let name_start = trimmed
                .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                .map_or(0, |at| at + 1);
            let name = &trimmed[name_start..];
            let name = if name.is_empty() { "name" } else { name };
            snippet.push_str(&format!("${{{}:{}}}", index + 1, name));
        }
    }
    snippet
}

/// Whether a new name for a hook can be written back into the string
/// literals that spell it: no quote or backslash to escape, and nothing a
/// double-quoted string or a pattern would read as interpolation.
pub(crate) fn is_valid_hook_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| matches!(c, '\'' | '"' | '\\' | '$' | '{' | '}') || c.is_control())
}

/// The files whose symbol maps hold a hook span, and the hooks the
/// installed WordPress packages fire.
#[derive(Default)]
pub(crate) struct HookFiles {
    uris: RwLock<HashSet<String>>,
    pub(super) dependencies: Mutex<Option<Arc<DependencyHooks>>>,
}

/// One hook span of some file: its byte range, its name, and whether it
/// fires the hook.
struct HookSpan<'a> {
    start: u32,
    end: u32,
    key: &'a str,
    fires: bool,
}

/// The hook spans of `map`.
fn hook_spans(map: &SymbolMap) -> impl Iterator<Item = HookSpan<'_>> {
    map.spans.iter().filter_map(|span| match &span.kind {
        SymbolKind::StringKey {
            kind: StringKeyKind::WordPressHook,
            key,
            is_write,
            ..
        } => Some(HookSpan {
            start: span.start,
            end: span.end,
            key,
            fires: *is_write,
        }),
        _ => None,
    })
}

impl Backend {
    /// Keep the set of files holding a hook span in step with a published
    /// symbol map.
    ///
    /// A dependency's hooks are left out, the way every other string key's
    /// are: only the files the project indexes as its own have a symbol map
    /// for each of them.
    pub(crate) fn note_wordpress_hooks(&self, uri: &str, map: &SymbolMap) {
        let holds = hook_spans(map).next().is_some()
            && !self
                .workspace
                .vendor_uri_prefixes
                .lock()
                .iter()
                .any(|prefix| uri.starts_with(prefix.as_str()));
        // Only take the write lock when the answer changes.
        if self.wordpress_hooks.uris.read().contains(uri) == holds {
            return;
        }
        let mut uris = self.wordpress_hooks.uris.write();
        if holds {
            uris.insert(uri.to_string());
        } else {
            uris.remove(uri);
        }
    }

    /// Forget a file that no longer exists.
    pub(crate) fn forget_wordpress_hooks(&self, uri: &str) {
        if self.wordpress_hooks.uris.read().contains(uri) {
            self.wordpress_hooks.uris.write().remove(uri);
        }
    }

    /// Whether the project file at `uri` holds a hook span.
    pub(crate) fn holds_wordpress_hooks(&self, uri: &str) -> bool {
        self.wordpress_hooks.uris.read().contains(uri)
    }

    /// Visit the hook spans of every file that holds one, file by file in a
    /// stable order: the project's own files, then the places the installed
    /// WordPress packages fire a hook. `external` is set for a file of one
    /// of the packages, which the project may index as its own too.
    fn visit_hook_files(&self, mut visit: impl FnMut(&str, bool, &[HookSpan<'_>])) {
        let dependencies = self.wordpress_dependency_hooks();
        let mut uris: Vec<String> = self.wordpress_hooks.uris.read().iter().cloned().collect();
        uris.sort_unstable();
        for uri in &uris {
            if let Some(map) = self.symbol_map_for(uri) {
                let spans: Vec<HookSpan<'_>> = hook_spans(&map).collect();
                visit(uri, dependencies.holds(uri), &spans);
            }
        }
        let mut spans = Vec::new();
        for (uri, firings) in &dependencies.files {
            if uris.binary_search(uri).is_ok() {
                continue;
            }
            spans.clear();
            spans.extend(firings.iter().map(|firing| HookSpan {
                start: firing.start,
                end: firing.end,
                key: firing.key.as_str(),
                fires: true,
            }));
            visit(uri, true, &spans);
        }
    }

    /// Whether the hooks core fires are read from files, the project's own
    /// or an installed package's, rather than taken from the list in the
    /// binary.
    fn reads_core_hooks(&self, dependencies: &DependencyHooks) -> bool {
        dependencies.has_core
            || self
                .wordpress_hooks
                .uris
                .read()
                .iter()
                .any(|uri| uri.contains("/wp-includes/"))
    }

    /// Whether something other than the project fires the hook `key`: an
    /// installed WordPress package, or core when the project has none to
    /// read. A rename of such a hook would leave its listeners behind.
    pub(crate) fn wordpress_hook_is_external(&self, key: &str) -> bool {
        let mut external = false;
        self.visit_hook_files(|_, is_external, spans| {
            external = external
                || is_external
                    && spans
                        .iter()
                        .any(|span| span.fires && hook_names_match(key, span.key));
        });
        external || self.wordpress_hook_is_listed(key)
    }

    /// Whether the list in the binary names the hook `key`, while the
    /// project has no core to read.
    fn wordpress_hook_is_listed(&self, key: &str) -> bool {
        !self.reads_core_hooks(&self.wordpress_dependency_hooks())
            && listed_core_hooks().any(|name| hook_names_match(key, name))
    }

    /// Every hook name the project fires or uses, or a dependency fires,
    /// sorted, leaving out the span that starts at `except` (a file and an
    /// offset): the one being typed.
    fn wordpress_hook_names(&self, except: Option<(&str, u32)>) -> Vec<String> {
        let mut names = Vec::new();
        self.visit_hook_files(|uri, _, spans| {
            for span in spans {
                if except != Some((uri, span.start)) {
                    names.push(span.key.to_string());
                }
            }
        });
        if !self.reads_core_hooks(&self.wordpress_dependency_hooks()) {
            names.extend(listed_core_hooks().map(str::to_string));
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The hook spans that match `key`, as locations, file by file.
    ///
    /// `fires` keeps the spans that fire the hook, `uses` the rest, and
    /// `literal_only` leaves out the patterns `key` only matches, which do
    /// not spell it.
    fn wordpress_hook_locations(
        &self,
        key: &str,
        fires: bool,
        uses: bool,
        literal_only: bool,
    ) -> Vec<Location> {
        let mut locations = Vec::new();
        self.visit_hook_files(|uri, _, spans| {
            let found: Vec<(u32, u32)> = spans
                .iter()
                .filter(|span| {
                    (if span.fires { fires } else { uses })
                        && (!literal_only || span.key == key)
                        && hook_names_match(key, span.key)
                })
                .map(|span| (span.start, span.end))
                .collect();
            if found.is_empty() {
                return;
            }
            // Only a file that holds a match has its text read.
            let (Some(parsed), Some(content)) =
                (crate::util::parse_url(uri), self.get_file_content_arc(uri))
            else {
                return;
            };
            let lines = LineIndex::new(&content);
            for (start, end) in found {
                locations.push(Location {
                    uri: parsed.clone(),
                    range: Range {
                        start: lines.position(start as usize),
                        end: lines.position(end as usize),
                    },
                });
            }
        });
        locations
    }

    /// Every place the project or a dependency fires the hook `key`, or a
    /// pattern it matches.
    pub(crate) fn wordpress_hook_definitions(&self, key: &str) -> Vec<Location> {
        self.wordpress_hook_locations(key, true, false, false)
    }

    /// Every place the project names the hook `key`, the places that fire
    /// it only when `include_declaration`.
    ///
    /// A rename (`literal_only`) only reaches the places that spell the name
    /// out: a pattern that matches it goes on matching the new one, or no
    /// longer fires it at all, and either way has no literal to rewrite.
    pub(crate) fn wordpress_hook_references(
        &self,
        key: &str,
        include_declaration: bool,
        literal_only: bool,
    ) -> Vec<Location> {
        self.wordpress_hook_locations(key, include_declaration, true, literal_only)
    }

    /// The other hook spans in `map` that name the hook `key`, firing ones
    /// as writes.
    pub(crate) fn wordpress_hook_highlights(
        &self,
        map: &SymbolMap,
        content: &str,
        key: &str,
    ) -> Vec<DocumentHighlight> {
        hook_spans(map)
            .filter(|span| hook_names_match(key, span.key))
            .map(|span| DocumentHighlight {
                range: Range {
                    start: offset_to_position(content, span.start as usize),
                    end: offset_to_position(content, span.end as usize),
                },
                kind: Some(if span.fires {
                    DocumentHighlightKind::Write
                } else {
                    DocumentHighlightKind::Read
                }),
            })
            .collect()
    }

    /// A summary of where the hook `key` fires and how often it is used,
    /// for hover.
    pub(crate) fn wordpress_hook_detail(&self, key: &str) -> String {
        let fired = self.wordpress_hook_definitions(key).len();
        let used = self.wordpress_hook_references(key, false, false).len();
        let count = |n: usize, one: &str, many: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {many}"),
        };
        if fired == 0 && self.wordpress_hook_is_listed(key) {
            format!("Fired by WordPress core, {}", count(used, "use", "uses"))
        } else if fired == 0 {
            format!("Not fired in the project, {}", count(used, "use", "uses"))
        } else {
            format!(
                "Fired in {}, {}",
                count(fired, "place", "places"),
                count(used, "use", "uses")
            )
        }
    }

    /// Complete the hook name a string literal passed to a hook function is
    /// being typed as: every name the project fires or uses or a
    /// dependency fires, and each
    /// pattern as a snippet with a placeholder for each computed part.
    pub(crate) fn try_wordpress_hook_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let cursor = position_to_offset(content, position) as usize;
        let (quote, _) = code.open_string?;
        let literal_start = quote + 1;
        let prefix = content.get(literal_start..cursor)?;
        let (callable, slot) = scalar_argument_at(content, code)?;
        match slot {
            ArgumentSlot::Positional(0) => {}
            ArgumentSlot::Named(name) if name == HOOK_PARAMETER => {}
            _ => return None,
        }
        hook_function_role(written_function(callable)?)?;

        let range = Range {
            start: offset_to_position(content, literal_start),
            end: position,
        };
        let items: Vec<CompletionItem> = self
            .wordpress_hook_names(Some((uri, literal_start as u32)))
            .into_iter()
            .filter(|name| name.starts_with(prefix))
            .enumerate()
            .map(|(index, name)| {
                let (new_text, format) = if is_pattern(&name) {
                    (pattern_snippet(&name), Some(InsertTextFormat::Snippet))
                } else {
                    (name.clone(), None)
                };
                CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::Event),
                    sort_text: Some(format!("{index:05}")),
                    filter_text: Some(name),
                    insert_text_format: format,
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range,
                        new_text,
                    })),
                    ..Default::default()
                }
            })
            .collect();
        (!items.is_empty()).then_some(CompletionResponse::CompletionItemList(items))
    }
}

/// The global function a call's text before its `(` names, as written
/// without a leading `\`: `None` for a method, a static call, a function
/// qualified with a namespace of its own, a function being declared, or
/// anything but a name.
fn written_function(callable: &str) -> Option<&str> {
    let is_name_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let callable = callable.trim_end();
    let bytes = callable.as_bytes();
    let mut start = bytes.len();
    while start > 0 && is_name_byte(bytes[start - 1]) {
        start -= 1;
    }
    let name = &callable[start..];
    if name.is_empty() || name.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    let before = &callable[..start];
    if let Some(qualifier) = before.strip_suffix('\\') {
        // `\add_action` is the global function, `Acme\add_action` another.
        return (!qualifier.ends_with(|c: char| is_name_byte(c as u8))).then_some(name);
    }
    let before = before.trim_end();
    let word_start = before
        .bytes()
        .rposition(|b| !is_name_byte(b))
        .map_or(0, |at| at + 1);
    let word = &before[word_start..];
    if before.ends_with("->")
        || before.ends_with("::")
        || before.ends_with('$')
        || word.eq_ignore_ascii_case("function")
        || word.eq_ignore_ascii_case("fn")
        || word.eq_ignore_ascii_case("new")
    {
        return None;
    }
    Some(name)
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
