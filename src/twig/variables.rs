//! Twig template variables: the names a template reads, the ones it sets
//! itself, and the ones the renders of it pass.
//!
//! A template is read as text (see [`analyse`]), like its filters and
//! functions (see [`super::callables`]): a bare name in an expression is a
//! variable, unless it is a filter, a function, a test, an attribute of
//! something, a key of a hash, a named argument, or one of the words of
//! the expression language.  A template defines a variable with `set`,
//! `for`, `with`, and `import`, a macro with its arguments, and declares
//! what it receives with the `types` tag.  A macro sees only its own
//! arguments and what it sets, never the template's context.
//!
//! What a template receives is found from the places that render it: the
//! keys of a PHP render's context, recorded with the render's template name
//! (see [`TwigContextSite`](crate::symbol_map::TwigContextSite)), and the
//! templates that include, embed, or extend it, which hand it the context
//! they have themselves along with the keys of their `with` hash.  The
//! project's globals (see
//! [`TwigVariable`](crate::symbol_map::StringKeyKind::TwigVariable)) reach
//! every template.  The answer is "what the callers that were found pass",
//! as for a Blade template's call-site variables: nothing is cached between
//! requests, and a request reads only the files that name the template.

use std::collections::HashSet;
use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::Location;

use super::callables::{
    is_identifier_byte, is_identifier_start, opens_raw_text, skip_test, tag_at, word_at,
};
use crate::Backend;
use crate::php_type::PhpType;
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan, TwigContextValue};
use crate::symfony::translations::{for_each_twig_block, skip_whitespace, string_end};

/// The words of Twig's expression language that are never a variable.
pub(super) const KEYWORDS: [&str; 24] = [
    "and",
    "or",
    "not",
    "in",
    "is",
    "xor",
    "matches",
    "starts",
    "ends",
    "with",
    "has",
    "some",
    "every",
    "if",
    "true",
    "false",
    "null",
    "none",
    "TRUE",
    "FALSE",
    "NULL",
    "NONE",
    "same",
    "divisible",
];

/// The variables Twig defines in every template.
const BUILTINS: [(&str, &str); 4] = [
    ("_self", "The template itself"),
    ("_context", "Every variable the template holds"),
    ("_charset", "The charset the environment renders with"),
    ("loop", "The state of the `for` loop it is written in"),
];

/// Whether `name` can be read as a variable in a template.
pub(crate) fn is_twig_variable_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.first().is_some_and(|byte| is_identifier_start(*byte))
        && bytes.iter().all(|byte| is_identifier_byte(*byte))
}

/// How a template defines a variable itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DefinitionKind {
    /// `{% set name = … %}`, with the byte range of the value it is set
    /// to, when the tag sets it to one.
    Set { value: Option<(usize, usize)> },
    /// A target of a `{% for key, value in … %}`: the byte range of what the
    /// loop iterates, and whether the target is the key.
    Loop { iterable: (usize, usize), key: bool },
    /// The argument of a `{% macro name(argument) %}`.
    MacroArgument,
    /// The name an `{% import '…' as name %}` gives the macros it imports.
    Import,
    /// A key of a `{% with {name: …} %}` hash, with the byte range of its
    /// value.
    With { value: Option<(usize, usize)> },
    /// A `{% types {name: 'type'} %}` entry: the type it declares, and
    /// whether the template does without it.
    Types { declared: String, optional: bool },
}

/// A variable a template defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Definition {
    /// The byte range of the name.
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) kind: DefinitionKind,
    /// The scope it is defined in: 0 for the template, and `n` for the
    /// body of [`TemplateVariables::macros`]`[n - 1]`.
    pub(crate) scope: usize,
}

/// A template another one loads by name, with what it hands it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Load {
    pub(crate) name: String,
    /// The byte range of the name, inside its quotes.
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// Whether the loaded template gets this one's context: an `include` or
    /// an `embed` without `only`, and the template this one `extends`.
    pub(crate) forwards: bool,
    /// The entries of the `with` hash.
    pub(crate) with: Vec<Passed>,
}

/// A key a hash passes on, and the byte range of its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Passed {
    pub(crate) key: (usize, usize),
    pub(crate) value: Option<(usize, usize)>,
}

/// What a template's text says about its variables.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TemplateVariables {
    /// The byte range of each name the template reads, in source order.
    pub(crate) uses: Vec<(usize, usize)>,
    pub(crate) definitions: Vec<Definition>,
    /// The byte range of each macro's body.
    pub(crate) macros: Vec<(usize, usize)>,
    pub(crate) loads: Vec<Load>,
}

impl TemplateVariables {
    /// The scope `offset` is in (see [`Definition::scope`]).
    pub(crate) fn scope_at(&self, offset: usize) -> usize {
        self.macros
            .iter()
            .position(|(start, end)| *start <= offset && offset <= *end)
            .map_or(0, |index| index + 1)
    }

    /// The definitions of `name` in `scope`.
    pub(crate) fn definitions_of<'s>(
        &'s self,
        content: &'s str,
        name: &'s str,
        scope: usize,
    ) -> impl Iterator<Item = &'s Definition> {
        self.definitions.iter().filter(move |definition| {
            definition.scope == scope && &content[definition.start..definition.end] == name
        })
    }
}

/// Read what `content`, a template, says about its variables.
pub(crate) fn analyse(content: &str) -> TemplateVariables {
    let mut scanner = Scanner {
        content,
        bytes: content.as_bytes(),
        out: TemplateVariables::default(),
        scope: 0,
    };
    if !content.contains("{{") && !content.contains("{%") {
        return scanner.out;
    }
    let mut raw_until: Option<&'static str> = None;
    for_each_twig_block(content, |body, end| {
        let tag = (scanner.bytes[body - 1] == b'%')
            .then(|| tag_at(content, body, end))
            .flatten();
        if let Some(close) = raw_until {
            if tag.is_some_and(|(tag, _)| tag == close) {
                raw_until = None;
            }
            return;
        }
        match tag {
            Some((tag, after)) => {
                if let Some(close) = opens_raw_text(tag) {
                    raw_until = Some(close);
                    return;
                }
                scanner.tag(tag, after, end);
            }
            None => scanner.expression(body, end, false, &[]),
        }
    });
    scanner.out
}

/// The spans of the variables a template reads and defines.
///
/// A definition is a read span too: the spans that declare a
/// [`StringKeyKind::TwigVariable`] are the project's globals, which every
/// template receives, and a template's own `set` reaches no other.
pub(crate) fn twig_variable_spans(content: &str) -> Vec<SymbolSpan> {
    let analysed = analyse(content);
    analysed
        .uses
        .iter()
        .copied()
        .chain(analysed.definitions.iter().map(|d| (d.start, d.end)))
        .map(|(start, end)| SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::TwigVariable,
                key: content[start..end].to_string(),
                is_write: false,
                is_optional: false,
            },
        })
        .collect()
}

struct Scanner<'c> {
    content: &'c str,
    bytes: &'c [u8],
    out: TemplateVariables,
    /// The scope the scan is in (see [`Definition::scope`]).
    scope: usize,
}

/// The keyword arguments of the tags that load a template.
const LOAD_KEYWORDS: [&str; 4] = ["with", "only", "ignore", "missing"];

impl Scanner<'_> {
    fn define(&mut self, start: usize, end: usize, kind: DefinitionKind) {
        if start < end && is_twig_variable_name(&self.content[start..end]) {
            self.out.definitions.push(Definition {
                start,
                end,
                kind,
                scope: self.scope,
            });
        }
    }

    /// The words of `start..end`, each as a byte range.
    fn words(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        let mut words = Vec::new();
        let mut cursor = start;
        while cursor < end {
            match word_at(self.bytes, cursor, end) {
                Some(word_end) => {
                    words.push((cursor, word_end));
                    cursor = word_end;
                }
                None => cursor += 1,
            }
        }
        words
    }

    /// Read the `{% tag … %}` whose arguments span `after..end`.
    fn tag(&mut self, tag: &str, after: usize, end: usize) {
        let content = self.content;
        let end = trim_end_marker(self.bytes, after, end);
        match tag {
            "set" => {
                let assignment = assignment_at(self.bytes, after, end);
                let targets = self.words(after, assignment.unwrap_or(end));
                // `{% set a, b = 1, 2 %}` sets each target to its own value.
                let values = assignment
                    .map(|assignment| top_level_parts(self.bytes, assignment + 1, end))
                    .filter(|values| values.len() == targets.len())
                    .unwrap_or_default();
                for (index, (start, word_end)) in targets.into_iter().enumerate() {
                    let value = values.get(index).copied();
                    self.define(start, word_end, DefinitionKind::Set { value });
                }
                if let Some(assignment) = assignment {
                    self.expression(assignment + 1, end, false, &[]);
                }
            }
            "for" => {
                let words = self.words(after, end);
                let Some(position) = words.iter().position(|(s, e)| &content[*s..*e] == "in")
                else {
                    return;
                };
                let iterable_start = words[position].1;
                // A Twig 2 `for … in … if …` filters what it iterates.
                let iterable_end = words[position..]
                    .iter()
                    .find(|(s, e)| &content[*s..*e] == "if")
                    .map_or(end, |(s, _)| *s);
                let iterable = trim_range(self.bytes, iterable_start, iterable_end);
                let targets = &words[..position];
                for (index, (start, word_end)) in targets.iter().enumerate() {
                    let key = targets.len() > 1 && index == 0;
                    self.define(*start, *word_end, DefinitionKind::Loop { iterable, key });
                }
                self.expression(words[position].1, end, false, &[]);
            }
            "macro" => {
                let name_start = skip_whitespace(self.bytes, after, end);
                let Some(name_end) = word_at(self.bytes, name_start, end) else {
                    return;
                };
                self.out.macros.push((end, self.bytes.len()));
                self.scope = self.out.macros.len();
                let Some(open) = (self.bytes.get(name_end) == Some(&b'(')).then_some(name_end)
                else {
                    return;
                };
                // Each argument is the first word of its part of the list,
                // and a default after it is a literal.
                let close = content[open..end].rfind(')').map_or(end, |at| open + at);
                let mut part_start = open + 1;
                for part in content[open + 1..close].split(',') {
                    let start = skip_whitespace(self.bytes, part_start, close);
                    if let Some(word_end) = word_at(self.bytes, start, close) {
                        self.define(start, word_end, DefinitionKind::MacroArgument);
                    }
                    part_start += part.len() + 1;
                }
            }
            "endmacro" => {
                if let Some(open) = self.scope.checked_sub(1)
                    && let Some(body) = self.out.macros.get_mut(open)
                {
                    body.1 = after;
                }
                self.scope = 0;
            }
            "import" => {
                let words = self.words(after, end);
                if let Some(position) = words.iter().rposition(|(s, e)| &content[*s..*e] == "as")
                    && let Some((start, word_end)) = words.get(position + 1)
                {
                    self.define(*start, *word_end, DefinitionKind::Import);
                }
            }
            "with" => {
                let open = skip_whitespace(self.bytes, after, end);
                for entry in hash_entries(self.bytes, open, end) {
                    let value = entry.value_range;
                    self.define(entry.key.0, entry.key.1, DefinitionKind::With { value });
                }
                self.expression(after, end, false, &["only"]);
            }
            "types" => {
                let open = skip_whitespace(self.bytes, after, end);
                for entry in hash_entries(self.bytes, open, end) {
                    let declared = entry
                        .value
                        .map(|(start, end)| content[start..end].replace("\\\\", "\\"))
                        .unwrap_or_default();
                    self.define(
                        entry.key.0,
                        entry.key.1,
                        DefinitionKind::Types {
                            declared,
                            optional: entry.optional,
                        },
                    );
                }
            }
            "include" | "embed" | "extends" => {
                self.record_load(after, end, tag == "extends");
                self.expression(after, end, false, &LOAD_KEYWORDS);
            }
            "if" | "elseif" | "do" | "dump" | "deprecated" | "stopwatch" => {
                self.expression(after, end, false, &[]);
            }
            "form_theme" => self.expression(after, end, false, &["with", "only"]),
            "trans" => self.expression(after, end, false, &["with", "from", "into"]),
            "apply" | "filter" => self.expression(after, end, true, &[]),
            // `{% block title page_title|title %}` is a block whose body is
            // the expression after its name.
            "block" => {
                let name_start = skip_whitespace(self.bytes, after, end);
                if let Some(name_end) = word_at(self.bytes, name_start, end) {
                    self.expression(name_end, end, false, &[]);
                }
            }
            // The other tags (`cache`, `use`, `from`, `guard`, the closing
            // ones, and the ones extensions add) are not read.
            _ => {}
        }
    }

    /// Record the template an `include`, `embed`, or `extends` tag whose
    /// arguments span `after..end` loads, when it names one literally.
    fn record_load(&mut self, after: usize, end: usize, extends: bool) {
        let open = skip_whitespace(self.bytes, after, end);
        let Some(quote @ (b'\'' | b'"')) = self.bytes.get(open).copied() else {
            return;
        };
        let Some(close) = string_end(self.bytes, open + 1, quote).filter(|close| *close < end)
        else {
            return;
        };
        let mut with = Vec::new();
        let mut only = false;
        let rest = close + 1;
        for (start, word_end) in self.words(rest, end) {
            match &self.content[start..word_end] {
                "with" => {
                    let open = skip_whitespace(self.bytes, word_end, end);
                    with = passed(self.bytes, open, end);
                }
                "only" => only = true,
                _ => {}
            }
        }
        self.out.loads.push(Load {
            name: self.content[open + 1..close].to_string(),
            start: open + 1,
            end: close,
            forwards: extends || !only,
            with,
        });
    }

    /// Record the template an `include('…', {…})` call whose `(` is at
    /// `open` loads.
    fn record_include_call(&mut self, open: usize, end: usize) {
        let name_start = skip_whitespace(self.bytes, open + 1, end);
        let Some(quote @ (b'\'' | b'"')) = self.bytes.get(name_start).copied() else {
            return;
        };
        let Some(close) = string_end(self.bytes, name_start + 1, quote).filter(|c| *c < end) else {
            return;
        };
        let after_name = skip_whitespace(self.bytes, close + 1, end);
        let mut with = Vec::new();
        let mut arguments_end = after_name;
        if self.bytes.get(after_name) == Some(&b',') {
            let hash = skip_whitespace(self.bytes, after_name + 1, end);
            with = passed(self.bytes, hash, end);
            arguments_end = match self.bytes.get(hash) {
                Some(b'{') => matching_close(self.bytes, hash, end).map_or(end, |close| close + 1),
                _ => hash,
            };
        }
        let call_end = matching_close(self.bytes, open, end).unwrap_or(end);
        // `with_context = false` (or a third argument of `false`) hands the
        // template only the hash.
        let forwards = !self.content[arguments_end.min(call_end)..call_end]
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|word| word == "false");
        self.out.loads.push(Load {
            name: self.content[name_start + 1..close].to_string(),
            start: name_start + 1,
            end: close,
            forwards,
            with,
        });
    }

    /// Record the variables the expression between `start` and `end`
    /// reads.  `filter_first` says that its first name is a filter, as an
    /// `apply` tag's is, and `keywords` are words the tag around it gives a
    /// meaning of its own.
    fn expression(&mut self, start: usize, end: usize, filter_first: bool, keywords: &[&str]) {
        let bytes = self.bytes;
        let content = self.content;
        let first_use = self.out.uses.len();
        // Each open bracket, whether it opens a call's arguments, and how
        // many names had been read before it.
        let mut brackets: Vec<(u8, bool, usize)> = Vec::new();
        let mut arrow_parameters: Vec<&str> = Vec::new();
        // The last significant thing read: a byte of punctuation, or `w`
        // for a variable, `n` for a name that is not one (a function, a
        // filter, an attribute), and `k` for a keyword or a literal.
        let mut previous = b'(';
        let mut expect_filter = filter_first;
        let mut cursor = start;
        while cursor < end {
            let byte = bytes[cursor];
            if byte.is_ascii_whitespace() {
                cursor += 1;
                continue;
            }
            if matches!(byte, b'\'' | b'"') {
                let Some(close) = string_end(bytes, cursor + 1, byte).filter(|c| *c < end) else {
                    break;
                };
                if byte == b'"' {
                    self.interpolations(cursor + 1, close);
                }
                cursor = close + 1;
                previous = b'k';
                continue;
            }
            if byte.is_ascii_digit() {
                while cursor < end && is_identifier_byte(bytes[cursor]) {
                    cursor += 1;
                }
                previous = b'k';
                continue;
            }
            if !is_identifier_start(byte) {
                match byte {
                    b'|' => expect_filter = true,
                    b'(' | b'[' | b'{' => {
                        let is_call = byte == b'(' && previous == b'n';
                        brackets.push((byte, is_call, self.out.uses.len()));
                    }
                    b')' | b']' | b'}' => {
                        let opened = brackets.pop();
                        let next = skip_whitespace(bytes, cursor + 1, end);
                        // `(a, b) => …` names the arrow function's
                        // parameters, not variables.
                        if byte == b')'
                            && bytes[next..end].starts_with(b"=>")
                            && let Some((_, _, before)) = opened
                        {
                            for (s, e) in self.out.uses.drain(before..) {
                                arrow_parameters.push(&content[s..e]);
                            }
                        }
                    }
                    _ => {}
                }
                previous = byte;
                cursor += 1;
                continue;
            }
            let word_end = word_at(bytes, cursor, end).unwrap_or(cursor + 1);
            let word = &content[cursor..word_end];
            let next = skip_whitespace(bytes, word_end, end);
            let next_byte = bytes.get(next).copied().filter(|_| next < end);
            if expect_filter {
                expect_filter = false;
                previous = b'n';
                cursor = word_end;
                continue;
            }
            if previous == b'.' && !(cursor >= 2 && bytes[cursor - 2] == b'.') {
                previous = b'n';
                cursor = word_end;
                continue;
            }
            if word == "is" {
                cursor = skip_test(bytes, word_end, end);
                previous = b'n';
                continue;
            }
            if bytes.get(word_end) == Some(&b'(') {
                if word == "include" {
                    self.record_include_call(word_end, end);
                }
                previous = b'n';
                cursor = word_end;
                continue;
            }
            // `b-and`, `b-or`, and `b-xor` are operators.
            if word == "b"
                && ["-and", "-or", "-xor"]
                    .iter()
                    .any(|op| content[word_end..end].starts_with(op))
            {
                previous = b'k';
                cursor = word_end;
                continue;
            }
            if KEYWORDS.contains(&word) || keywords.contains(&word) {
                previous = b'k';
                cursor = word_end;
                continue;
            }
            let top = brackets.last().copied();
            let at_entry_start = matches!(previous, b'{' | b'(' | b',');
            // A hash key, `{name: …}`.
            let is_key =
                next_byte == Some(b':') && at_entry_start && matches!(top, Some((b'{', _, _)));
            // A named argument, `f(name: …)` or `f(name = …)`.
            let is_named_argument = at_entry_start
                && matches!(top, Some((b'(', true, _)))
                && (next_byte == Some(b':')
                    || (next_byte == Some(b'=')
                        && !matches!(bytes.get(next + 1), Some(b'=' | b'>'))));
            if is_key || is_named_argument {
                previous = b'k';
                cursor = word_end;
                continue;
            }
            if bytes[next..end].starts_with(b"=>") {
                arrow_parameters.push(word);
                previous = b'k';
                cursor = word_end;
                continue;
            }
            self.out.uses.push((cursor, word_end));
            previous = b'w';
            cursor = word_end;
        }
        if !arrow_parameters.is_empty() {
            let mut index = first_use;
            while index < self.out.uses.len() {
                let (s, e) = self.out.uses[index];
                if arrow_parameters.contains(&&content[s..e]) {
                    self.out.uses.remove(index);
                } else {
                    index += 1;
                }
            }
        }
    }

    /// Read the `#{…}` interpolations of a double-quoted string whose
    /// contents span `start..end`.
    fn interpolations(&mut self, start: usize, end: usize) {
        let mut cursor = start;
        while let Some(relative) = self.content[cursor..end].find("#{") {
            let open = cursor + relative + 1;
            let close = matching_close(self.bytes, open, end).unwrap_or(end);
            self.expression(open + 1, close, false, &[]);
            cursor = (close + 1).min(end);
        }
    }
}

/// Where the arguments of a tag spanning `start..end` end, before a `-` or
/// `~` that trims the whitespace after it.
fn trim_end_marker(bytes: &[u8], start: usize, end: usize) -> usize {
    if end > start && matches!(bytes[end - 1], b'-' | b'~') {
        end - 1
    } else {
        end
    }
}

/// The `=` of a `set` tag spanning `start..end`, outside its strings.
fn assignment_at(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < end {
        match bytes[cursor] {
            quote @ (b'\'' | b'"') => cursor = string_end(bytes, cursor + 1, quote)? + 1,
            b'=' if !matches!(bytes.get(cursor + 1), Some(b'=' | b'>')) => return Some(cursor),
            _ => cursor += 1,
        }
    }
    None
}

/// The bracket that closes the one at `open`, past strings and the
/// brackets inside it.
fn matching_close(bytes: &[u8], open: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = open;
    while cursor < end {
        match bytes[cursor] {
            quote @ (b'\'' | b'"') => cursor = string_end(bytes, cursor + 1, quote)?,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

/// One entry of a hash literal.
struct HashEntry {
    /// The byte range of the key's name, inside its quotes if it has them.
    key: (usize, usize),
    /// Whether the key is marked `?`, as a `types` entry the template does
    /// without is.
    optional: bool,
    /// The byte range of the value's contents, when it is a string.
    value: Option<(usize, usize)>,
    /// The byte range of the value.
    value_range: Option<(usize, usize)>,
}

/// The entries of the hash literal whose `{` is at `open`, as a `with` hash
/// passes them on.
fn passed(bytes: &[u8], open: usize, end: usize) -> Vec<Passed> {
    hash_entries(bytes, open, end)
        .into_iter()
        .map(|entry| Passed {
            key: entry.key,
            value: entry.value_range,
        })
        .collect()
}

/// `start..end` without the whitespace around it.
pub(crate) fn trim_range(bytes: &[u8], start: usize, end: usize) -> (usize, usize) {
    let start = skip_whitespace(bytes, start, end);
    let mut trimmed_end = end;
    while trimmed_end > start && bytes[trimmed_end - 1].is_ascii_whitespace() {
        trimmed_end -= 1;
    }
    (start, trimmed_end)
}

/// The parts of `start..end` the commas outside its brackets and strings
/// separate, each trimmed.
fn top_level_parts(bytes: &[u8], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut part_start = start;
    let mut cursor = start;
    while cursor < end {
        match bytes[cursor] {
            quote @ (b'\'' | b'"') => match string_end(bytes, cursor + 1, quote) {
                Some(close) => cursor = close,
                None => break,
            },
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(trim_range(bytes, part_start, cursor));
                part_start = cursor + 1;
            }
            _ => {}
        }
        cursor += 1;
    }
    parts.push(trim_range(bytes, part_start, end));
    parts
}

/// The entries of the hash literal whose `{` is at `open`.
fn hash_entries(bytes: &[u8], open: usize, end: usize) -> Vec<HashEntry> {
    let mut entries = Vec::new();
    if bytes.get(open) != Some(&b'{') {
        return entries;
    }
    let close = matching_close(bytes, open, end).unwrap_or(end);
    let mut cursor = open + 1;
    while cursor < close {
        cursor = skip_whitespace(bytes, cursor, close);
        let key = match bytes.get(cursor) {
            Some(quote @ (b'\'' | b'"')) => match string_end(bytes, cursor + 1, *quote) {
                Some(key_end) if key_end < close => {
                    let key = (cursor + 1, key_end);
                    cursor = key_end + 1;
                    Some(key)
                }
                _ => return entries,
            },
            Some(byte) if is_identifier_start(*byte) => {
                let key_end = word_at(bytes, cursor, close).unwrap_or(cursor + 1);
                let key = (cursor, key_end);
                cursor = key_end;
                Some(key)
            }
            _ => None,
        };
        cursor = skip_whitespace(bytes, cursor, close);
        let optional = bytes.get(cursor) == Some(&b'?');
        if optional {
            cursor = skip_whitespace(bytes, cursor + 1, close);
        }
        let has_value = bytes.get(cursor) == Some(&b':');
        let value_start = skip_whitespace(bytes, cursor + usize::from(has_value), close);
        let value = match bytes.get(value_start) {
            Some(quote @ (b'\'' | b'"')) if has_value => string_end(bytes, value_start + 1, *quote)
                .filter(|value_end| *value_end < close)
                .map(|value_end| (value_start + 1, value_end)),
            _ => None,
        };
        let entry_index = key.map(|key| {
            entries.push(HashEntry {
                key,
                optional,
                value,
                value_range: None,
            });
            entries.len() - 1
        });
        // On to the `,` after the value.
        let mut depth = 0usize;
        while cursor < close {
            match bytes[cursor] {
                quote @ (b'\'' | b'"') => match string_end(bytes, cursor + 1, quote) {
                    Some(string_close) => cursor = string_close,
                    None => return entries,
                },
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => break,
                _ => {}
            }
            cursor += 1;
        }
        if has_value
            && let Some(index) = entry_index
            && value_start < cursor
        {
            entries[index].value_range = Some(trim_range(bytes, value_start, cursor));
        }
        cursor += 1;
    }
    entries
}

// ─── What a template receives ──────────────────────────────────────────────

/// Where a variable a template receives comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OriginKind {
    /// A key of the context a PHP render passes.
    Render,
    /// A template that includes, embeds, or extends this one, through the
    /// key of its `with` hash or the variable it defines itself.
    Template,
    /// A global the project declares.
    Global,
}

/// One place a variable a template receives comes from: the file, and the
/// byte range of the name there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origin {
    pub(crate) name: String,
    pub(crate) kind: OriginKind,
    pub(crate) uri: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
    /// Where the value it holds is written.
    pub(crate) value: OriginValue,
}

/// Where the value a variable a template receives holds is written, which
/// is what its type is read from (see [`super::types`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OriginValue {
    /// Nowhere that can be read, as for a global a YAML file declares.
    Unknown,
    /// A value of a PHP render's context.
    Php(TwigContextValue),
    /// The value written beside the key in a PHP file: a global's.
    PhpBeside,
    /// An entry of a PHP context only its type names, already read off it.
    Type(PhpType),
    /// The expression spanning `start..end` of the template that passes it.
    Twig { start: usize, end: usize },
    /// The variable as the template that passes it on defines it.
    TwigDefinition,
}

/// What a variable written in a template is.
#[derive(Debug)]
pub(super) enum Resolved {
    /// The template defines it, in the scope it is written in.
    Local(Vec<Definition>),
    /// It is received: declared by the template's `types` tag, passed by
    /// the renders that were found, or a global.
    Received {
        declared: Vec<Definition>,
        origins: Vec<Origin>,
    },
    /// One of Twig's own, or of the framework's.
    Builtin(&'static str),
}

impl Backend {
    pub(super) fn twig_template_text(&self, uri: &str) -> Option<Arc<String>> {
        self.get_file_content_arc(uri).or_else(|| {
            crate::util::uri_to_path(uri)
                .and_then(|path| std::fs::read_to_string(path).ok())
                .map(Arc::new)
        })
    }

    /// The globals the project declares.
    pub(super) fn twig_global_origins(&self) -> Vec<Origin> {
        let mut origins = Vec::new();
        self.visit_symfony_declarations(StringKeyKind::TwigVariable, |uri, _, start, end, key| {
            let value = if uri.ends_with(".php") {
                OriginValue::PhpBeside
            } else {
                OriginValue::Unknown
            };
            origins.push(Origin {
                name: key.to_string(),
                kind: OriginKind::Global,
                uri: uri.to_string(),
                start,
                end,
                value,
            });
        });
        origins
    }

    /// The globals the framework defines itself, with what each holds.
    fn twig_framework_globals(&self) -> &'static [(&'static str, &'static str)] {
        if self.is_symfony_project() {
            &[(
                "app",
                "Symfony's `AppVariable`: the request, the user, the session, and the flash messages",
            )]
        } else {
            &[]
        }
    }

    /// Every variable the template at `uri` is handed by the renders and
    /// the templates that load it, each with where it comes from.
    pub(crate) fn twig_received_variables(&self, uri: &str) -> Vec<Origin> {
        self.ensure_workspace_index_ready_for_request();
        let mut visited = HashSet::new();
        let mut origins = Vec::new();
        self.collect_twig_received(uri, &mut visited, &mut origins);
        let mut seen = HashSet::new();
        origins
            .retain(|origin| seen.insert((origin.name.clone(), origin.uri.clone(), origin.start)));
        origins
    }

    fn collect_twig_received(
        &self,
        uri: &str,
        visited: &mut HashSet<String>,
        origins: &mut Vec<Origin>,
    ) {
        if !visited.insert(uri.to_string()) {
            return;
        }
        let roots = self.twig_roots();
        let names = crate::util::uri_to_path(uri)
            .map(|path| roots.template_names(&path))
            .unwrap_or_default();
        if names.is_empty() {
            return;
        }
        let keys: Vec<crate::reference_index::ReferenceIndexKey> = names
            .iter()
            .map(
                |name| crate::reference_index::ReferenceIndexKey::StringKey {
                    kind: StringKeyKind::TwigTemplate,
                    key: name.clone(),
                },
            )
            .collect();
        let mut snapshot = self.user_file_symbol_maps_for_reference_keys(&keys);
        snapshot.sort_by(|a, b| a.0.cmp(&b.0));
        let names_this = |key: &str| names.iter().any(|name| name == key);
        for (file_uri, map) in snapshot {
            if file_uri == uri {
                continue;
            }
            if self.is_twig_template_uri(&file_uri) {
                let loads_this = map.spans.iter().any(|span| {
                    matches!(
                        &span.kind,
                        SymbolKind::StringKey {
                            kind: StringKeyKind::TwigTemplate,
                            key,
                            is_write: false,
                            ..
                        } if names_this(key)
                    )
                });
                if !loads_this {
                    continue;
                }
                let Some(content) = self.twig_template_text(&file_uri) else {
                    continue;
                };
                let analysed = analyse(&content);
                let mut forwards = false;
                for load in analysed.loads.iter().filter(|load| names_this(&load.name)) {
                    forwards |= load.forwards;
                    for passed in &load.with {
                        let (start, end) = passed.key;
                        origins.push(Origin {
                            name: content[start..end].to_string(),
                            kind: OriginKind::Template,
                            uri: file_uri.clone(),
                            start: start as u32,
                            end: end as u32,
                            value: passed.value.map_or(OriginValue::Unknown, |(start, end)| {
                                OriginValue::Twig { start, end }
                            }),
                        });
                    }
                }
                if forwards {
                    for definition in analysed.definitions.iter().filter(|d| d.scope == 0) {
                        origins.push(Origin {
                            name: content[definition.start..definition.end].to_string(),
                            kind: OriginKind::Template,
                            uri: file_uri.clone(),
                            start: definition.start as u32,
                            end: definition.end as u32,
                            value: OriginValue::TwigDefinition,
                        });
                    }
                    self.collect_twig_received(&file_uri, visited, origins);
                }
                continue;
            }
            if map.twig_contexts.is_empty() {
                continue;
            }
            // A render whose receiver only a type settles is not in the map,
            // so ask for the file's confirmed extras, but only when one of
            // its candidates names this template.
            let has_candidate = map
                .typed_receiver_sites
                .iter()
                .any(|site| site.receiver.names_twig_templates() && names_this(&site.key));
            let extra = if has_candidate {
                self.typed_receiver_spans_for(&file_uri, &map)
            } else {
                Arc::new(Vec::new())
            };
            let starts: Vec<u32> = map
                .spans
                .iter()
                .chain(extra.iter())
                .filter(|span| {
                    matches!(
                        &span.kind,
                        SymbolKind::StringKey {
                            kind: StringKeyKind::TwigTemplate,
                            key,
                            is_write: false,
                            ..
                        } if names_this(key)
                    )
                })
                .map(|span| span.start)
                .collect();
            let sites: Vec<&crate::symbol_map::TwigContextSite> = map
                .twig_contexts
                .iter()
                .filter(|site| starts.contains(&site.template_start))
                .collect();
            for site in &sites {
                origins.extend(site.vars.iter().map(|var| Origin {
                    name: var.name.to_string(),
                    kind: OriginKind::Render,
                    uri: file_uri.clone(),
                    start: var.start,
                    end: var.end,
                    value: OriginValue::Php(var.value),
                }));
            }
            // A context passed as a whole names its entries by its type.
            let shapes: Vec<(u32, u32)> = sites.iter().filter_map(|site| site.shape).collect();
            if !shapes.is_empty() {
                for ((start, end), entries) in shapes
                    .iter()
                    .zip(self.twig_context_shapes(&file_uri, &shapes))
                {
                    origins.extend(entries.into_iter().map(|(name, ty)| Origin {
                        name,
                        kind: OriginKind::Render,
                        uri: file_uri.clone(),
                        start: *start,
                        end: *end,
                        value: OriginValue::Type(ty),
                    }));
                }
            }
        }
    }

    /// What the variable `name`, written at `offset` in the template at
    /// `uri`, is.  Without an offset, it is read as written in the
    /// template's own scope rather than a macro's.
    pub(super) fn resolve_twig_variable(
        &self,
        uri: &str,
        offset: Option<usize>,
        name: &str,
    ) -> Resolved {
        let analysed = self
            .twig_template_text(uri)
            .map(|content| {
                let analysed = analyse(&content);
                let scope = offset.map_or(0, |offset| analysed.scope_at(offset));
                let definitions: Vec<Definition> = analysed
                    .definitions_of(&content, name, scope)
                    .cloned()
                    .collect();
                (scope, definitions)
            })
            .unwrap_or_default();
        let (scope, definitions) = analysed;
        let (declared, local): (Vec<Definition>, Vec<Definition>) = definitions
            .into_iter()
            .partition(|d| matches!(d.kind, DefinitionKind::Types { .. }));
        if !local.is_empty() {
            return Resolved::Local(local);
        }
        let mut origins: Vec<Origin> = Vec::new();
        // A macro is handed only its arguments.
        if scope == 0 {
            origins.extend(
                self.twig_received_variables(uri)
                    .into_iter()
                    .filter(|origin| origin.name == name),
            );
        }
        origins.extend(
            self.twig_global_origins()
                .into_iter()
                .filter(|origin| origin.name == name),
        );
        if declared.is_empty() && origins.is_empty() {
            let builtin = BUILTINS
                .iter()
                .chain(self.twig_framework_globals())
                .find(|(builtin, _)| *builtin == name);
            if let Some((_, description)) = builtin {
                return Resolved::Builtin(description);
            }
        }
        Resolved::Received { declared, origins }
    }

    fn location_in(&self, uri: &str, start: usize, end: usize) -> Option<Location> {
        let content = self.twig_template_text(uri)?;
        Some(Location {
            uri: crate::util::parse_url(uri)?,
            range: crate::text_position::byte_range_to_lsp_range(&content, start, end),
        })
    }

    /// Where the variable `name` written at `offset` in the file at `uri`
    /// comes from: the template's own definition of it, or the keys that
    /// pass it and the globals that declare it.  In a file other than a
    /// template, `name` is a global, declared where the project declares it.
    pub(crate) fn twig_variable_definitions(
        &self,
        uri: &str,
        offset: Option<usize>,
        name: &str,
    ) -> Vec<Location> {
        let mut locations: Vec<Location> = Vec::new();
        let mut push = |location: Option<Location>| {
            if let Some(location) = location
                && !locations.contains(&location)
            {
                locations.push(location);
            }
        };
        if !self.is_twig_template_uri(uri) {
            for origin in self.twig_global_origins() {
                if origin.name == name {
                    push(self.location_in(&origin.uri, origin.start as usize, origin.end as usize));
                }
            }
            return locations;
        }
        match self.resolve_twig_variable(uri, offset, name) {
            Resolved::Local(definitions) => {
                for definition in definitions {
                    push(self.location_in(uri, definition.start, definition.end));
                }
            }
            Resolved::Received { declared, origins } => {
                for definition in declared {
                    push(self.location_in(uri, definition.start, definition.end));
                }
                for origin in origins {
                    push(self.location_in(&origin.uri, origin.start as usize, origin.end as usize));
                }
            }
            Resolved::Builtin(_) => {}
        }
        locations
    }

    /// The places that read or define the variable `name` written at
    /// `offset` in the template at `uri`: the template's own spellings of it
    /// in the scope it is written in, and the keys that pass it in.
    pub(crate) fn twig_variable_references(
        &self,
        uri: &str,
        offset: Option<usize>,
        name: &str,
    ) -> Vec<Location> {
        let Some(content) = self.twig_template_text(uri) else {
            return Vec::new();
        };
        let analysed = analyse(&content);
        let scope = offset.map_or(0, |offset| analysed.scope_at(offset));
        let mut locations: Vec<Location> = Vec::new();
        let spellings = analysed
            .uses
            .iter()
            .copied()
            .chain(analysed.definitions.iter().map(|d| (d.start, d.end)))
            .filter(|(start, end)| {
                &content[*start..*end] == name && analysed.scope_at(*start) == scope
            });
        let mut spellings: Vec<(usize, usize)> = spellings.collect();
        spellings.sort_unstable();
        if let Some(parsed) = crate::util::parse_url(uri) {
            for (start, end) in spellings {
                locations.push(Location {
                    uri: parsed.clone(),
                    range: crate::text_position::byte_range_to_lsp_range(&content, start, end),
                });
            }
        }
        if let Resolved::Received { origins, .. } = self.resolve_twig_variable(uri, offset, name) {
            for origin in origins {
                if let Some(location) =
                    self.location_in(&origin.uri, origin.start as usize, origin.end as usize)
                    && !locations.contains(&location)
                {
                    locations.push(location);
                }
            }
        }
        locations
    }

    /// What the hover of the variable `name` written at `offset` in the file
    /// at `uri` says below its name.
    pub(crate) fn twig_variable_detail(
        &self,
        uri: &str,
        offset: Option<usize>,
        name: &str,
    ) -> String {
        let detail = self.twig_variable_origin_detail(uri, offset, name);
        let ty = offset
            .filter(|_| self.is_twig_template_uri(uri))
            .and_then(|offset| self.twig_variable_type(uri, offset, name));
        match ty {
            Some(ty) => format!("`{}`\n\n{detail}", super::types::display_type(&ty)),
            None => detail,
        }
    }

    /// Where the variable `name` written at `offset` in the file at `uri`
    /// comes from, as its hover says it.
    fn twig_variable_origin_detail(&self, uri: &str, offset: Option<usize>, name: &str) -> String {
        if !self.is_twig_template_uri(uri) {
            return "A global every template receives".to_string();
        }
        let relative = |uri: &str| {
            self.workspace_relative_path(uri)
                .unwrap_or_else(|| uri.to_string())
        };
        match self.resolve_twig_variable(uri, offset, name) {
            Resolved::Local(definitions) => {
                let mut lines: Vec<&str> = definitions
                    .iter()
                    .map(|definition| match definition.kind {
                        DefinitionKind::Set { .. } => "Set by this template",
                        DefinitionKind::Loop { .. } => "Set by a `for` loop of this template",
                        DefinitionKind::MacroArgument => "An argument of the macro",
                        DefinitionKind::Import => "The macros this template imports",
                        DefinitionKind::With { .. } => "Set by a `with` tag of this template",
                        DefinitionKind::Types { .. } => "Declared by this template",
                    })
                    .collect();
                lines.dedup();
                lines.join("\n\n")
            }
            Resolved::Builtin(description) => description.to_string(),
            Resolved::Received { declared, origins } => {
                let mut lines: Vec<String> = Vec::new();
                for definition in &declared {
                    if let DefinitionKind::Types { declared, optional } = &definition.kind {
                        lines.push(format!(
                            "Declared as `{declared}`{} by this template's `types` tag",
                            if *optional { ", optional," } else { "" }
                        ));
                    }
                }
                for origin in &origins {
                    let line = match origin.kind {
                        OriginKind::Render => format!("Passed by `{}`", relative(&origin.uri)),
                        OriginKind::Template => {
                            format!("Passed on by `{}`", relative(&origin.uri))
                        }
                        OriginKind::Global => {
                            format!("A global, declared in `{}`", relative(&origin.uri))
                        }
                    };
                    if !lines.contains(&line) {
                        lines.push(line);
                    }
                }
                if lines.is_empty() {
                    "Not passed by any render that was found".to_string()
                } else {
                    lines.join("\n\n")
                }
            }
        }
    }

    /// The variables that can be read at `cursor` in the template at `uri`,
    /// whose text is `content`, sorted.
    pub(crate) fn twig_variable_names(
        &self,
        uri: &str,
        content: &str,
        cursor: usize,
    ) -> Vec<String> {
        let analysed = analyse(content);
        let scope = analysed.scope_at(cursor);
        let mut names: Vec<String> = analysed
            .definitions
            .iter()
            .filter(|d| d.scope == scope && !(d.start <= cursor && cursor <= d.end))
            .map(|d| content[d.start..d.end].to_string())
            .collect();
        if scope == 0 {
            names.extend(
                self.twig_received_variables(uri)
                    .into_iter()
                    .map(|origin| origin.name),
            );
        }
        names.extend(
            self.twig_global_origins()
                .into_iter()
                .map(|origin| origin.name),
        );
        names.extend(
            BUILTINS
                .iter()
                .chain(self.twig_framework_globals())
                .filter(|(name, _)| *name != "loop")
                .map(|(name, _)| name.to_string()),
        );
        names.sort_unstable();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uses(content: &str) -> Vec<&str> {
        analyse(content)
            .uses
            .iter()
            .map(|(start, end)| &content[*start..*end])
            .collect()
    }

    fn definitions(content: &str) -> Vec<String> {
        analyse(content)
            .definitions
            .iter()
            .map(|d| {
                let name = &content[d.start..d.end];
                match &d.kind {
                    DefinitionKind::Types { declared, optional } => {
                        format!("{name}{}: {declared}", if *optional { "?" } else { "" })
                    }
                    kind => {
                        let kind = format!("{kind:?}");
                        let kind = kind.split(' ').next().unwrap_or_default();
                        format!("{name} {kind} {}", d.scope)
                    }
                }
            })
            .collect()
    }

    #[test]
    fn names_that_are_not_variables_are_left_out() {
        let twig = "{{ post.title|upper }} {{ max(a, b) }} {{ x is defined ? y : z }}\n{{ {key: value, 'q': w} }} {{ f(name: n, other = o) }} {{ true and not none }}\n{{ items|map(i => i.name)|filter((k, v) => v > limit) }} {{ \"Hi #{user.name}!\" }}\n{{ 1..count }} {{ m.input() }} {{ a b-and c }} {# {{ commented }} #}{% verbatim %}{{ raw }}{% endverbatim %}";
        assert_eq!(
            uses(twig),
            [
                "post", "a", "b", "x", "y", "z", "value", "w", "n", "o", "items", "limit", "user",
                "count", "m", "a", "c",
            ]
        );
    }

    #[test]
    fn tags_read_their_expressions_and_define_their_targets() {
        let twig = "{% set total = price * qty %}{% set a, b = 1, 2 %}{% set body %}x{% endset %}\n{% for key, item in items if item.ok %}{{ loop.index }}{% endfor %}\n{% if user %}{% elseif guest %}{% endif %}{% import 'forms.html.twig' as forms %}\n{% include 'card.html.twig' with {card: post} only %}{% with {inner: outer} %}{% endwith %}\n{% block title page_title|title %}{% apply upper %}{% endapply %}";
        assert_eq!(
            uses(twig),
            [
                "price",
                "qty",
                "items",
                "item",
                "loop",
                "user",
                "guest",
                "post",
                "outer",
                "page_title"
            ]
        );
        assert_eq!(
            definitions(twig),
            [
                "total Set 0",
                "a Set 0",
                "b Set 0",
                "body Set 0",
                "key Loop 0",
                "item Loop 0",
                "forms Import 0",
                "inner With 0",
            ]
        );
    }

    #[test]
    fn macros_have_scopes_of_their_own() {
        let twig = "{{ outside }}{% macro row(name, value = 'x') %}{{ name }}{{ value }}{% set local = 1 %}{% endmacro %}{{ after }}";
        let analysed = analyse(twig);
        assert_eq!(
            definitions(twig),
            [
                "name MacroArgument 1",
                "value MacroArgument 1",
                "local Set 1"
            ]
        );
        let at = |text: &str| analysed.scope_at(twig.find(text).unwrap());
        assert_eq!(at("outside"), 0);
        assert_eq!(at("name }}"), 1);
        assert_eq!(at("after"), 0);
    }

    #[test]
    fn types_declare_what_the_template_receives() {
        let twig = "{% types {is_correct: 'bool', score?: 'int', 'user': '\\\\App\\\\User'} %}";
        assert_eq!(
            definitions(twig),
            ["is_correct: bool", "score?: int", "user: \\App\\User"]
        );
    }

    #[test]
    fn loads_say_what_they_hand_the_template() {
        let twig = "{% extends 'base.html.twig' %}{% include 'a.html.twig' with {x: 1} %}{% include 'b.html.twig' only %}{{ include('c.html.twig', {y: 2}, with_context = false) }}{% embed 'd.html.twig' %}{% endembed %}";
        let analysed = analyse(twig);
        let loads: Vec<(String, bool, Vec<&str>)> = analysed
            .loads
            .iter()
            .map(|load| {
                assert_eq!(&twig[load.start..load.end], load.name);
                (
                    load.name.clone(),
                    load.forwards,
                    load.with.iter().map(|p| &twig[p.key.0..p.key.1]).collect(),
                )
            })
            .collect();
        assert_eq!(
            loads,
            [
                ("base.html.twig".to_string(), true, vec![]),
                ("a.html.twig".to_string(), true, vec!["x"]),
                ("b.html.twig".to_string(), false, vec![]),
                ("c.html.twig".to_string(), false, vec!["y"]),
                ("d.html.twig".to_string(), true, vec![]),
            ]
        );
    }
}
