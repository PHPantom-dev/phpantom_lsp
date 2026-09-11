//! Reindent-only formatting for Blade templates.
//!
//! Every line keeps its content and its line breaks; only the leading
//! whitespace changes. That is the one thing every Blade formatter agrees
//! on, it needs no per-language formatter for the CSS, JavaScript, and
//! PHP embedded in a template, and it cannot change what a template
//! renders, since HTML collapses leading whitespace everywhere except in
//! the few places this formatter leaves untouched.
//!
//! The model is a stack of open constructs. Each one adds a level of
//! indentation to the lines inside it:
//!
//! - a block directive (`@if` … `@endif`), including any pair the
//!   template itself defines by writing an `@end…` for it;
//! - an HTML or component element with a body (`<div>` … `</div>`), except
//!   `<html>`, and except an inline element or component that is followed
//!   by text on its own line, which is prose rather than structure;
//! - the attribute list of an opening tag that spans several lines, with
//!   the `>` or `/>` back at the tag's own level;
//! - a `{`, `[`, or `(` left open at the end of a line, in directive
//!   arguments, echoes, attribute values, and plain text alike, so the
//!   contents of a multi-line `@props([`, `x-data="{`, or `@if (` are
//!   indented and the closing bracket returns to the opener's level;
//! - an attribute value that runs across lines.
//!
//! A `@case` body is one more level inside its `@switch`, `@else` and its
//! relatives sit at their `@if`'s level, and a closer that is the first
//! thing on a line is written at the level of the construct it closes. A
//! block that is closed while an element opened inside it is still open,
//! the conditional-wrapper idiom, aligns the closer with its opener and
//! remembers the element so that its eventual closing tag changes nothing.
//!
//! Bodies of `<script>`, `<style>`, `@php`, `<?php … ?>`, and multi-line
//! comments are shifted as a block to the enclosing level with their own
//! relative indentation kept. Bodies of `<pre>`, `<textarea>`, and
//! `@verbatim`, and the lines between `{{-- blade-formatter-disable --}}`
//! and `{{-- blade-formatter-enable --}}`, are left byte for byte, their
//! closing line included: whitespace there is output.
//!
//! Regions Blade ignores directives in are read the way `super::super::
//! blade::balance` reads them, and the block table is the same one the
//! unbalanced-directive diagnostic checks against, so the formatter and
//! the diagnostic cannot disagree about what closes what.

use std::collections::HashSet;
use std::ops::Range;

use crate::blade::balance::{BLOCKS, opens_block};
use crate::blade::component_tags::{is_attr_name_char, is_tag_name_char};
use crate::blade::signature::{matching_paren, skip_php_comment};

/// How the formatter lays a template out, from the editor's formatting
/// options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// One level of indentation.
    pub indent: String,
    /// Strip spaces and tabs from the end of every reindented line.
    pub trim_trailing_whitespace: bool,
    /// End the template with a line break when it does not already.
    pub insert_final_newline: bool,
    /// Reduce a run of blank lines at the end of the template to one
    /// line break.
    pub trim_final_newlines: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            indent: "    ".to_string(),
            trim_trailing_whitespace: false,
            insert_final_newline: false,
            trim_final_newlines: false,
        }
    }
}

/// Elements that never have a closing tag.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose children are not indented.
const UNINDENTED_ELEMENTS: &[&str] = &["html"];

/// Elements that flow with text, so one followed by text on its line is
/// prose and opens nothing.
const INLINE_ELEMENTS: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "dfn", "em", "i", "kbd", "mark", "q",
    "rp", "rt", "ruby", "s", "samp", "small", "span", "strong", "sub", "sup", "time", "u", "var",
];

/// Elements whose body is kept as it is: their whitespace is rendered
/// (`<pre>`, `<textarea>`).
pub(super) const PRESERVED_ELEMENTS: &[&str] = &["pre", "textarea"];

/// Elements whose body is another language, shifted as a block rather
/// than reindented line by line.
pub(super) const OPAQUE_ELEMENTS: &[&str] = &["script", "style"];

pub(super) const DISABLE_MARKER: &str = "blade-formatter-disable";
pub(super) const ENABLE_MARKER: &str = "blade-formatter-enable";

/// Reindent `content` with `options`.
pub fn reindent(content: &str, options: &Options) -> String {
    let events = Scanner::new(content).scan();
    let mut out = Walker::new(content, options).walk(&events);
    finish_file(&mut out, content, options);
    out
}

// ── Events ──────────────────────────────────────────────────────────

/// What a stretch of a template between an opener and its terminator is
/// to the formatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegionKind {
    /// Byte for byte, its closing line included.
    Preserve,
    /// Shifted as a block to one level inside the opener, with its own
    /// relative indentation kept.
    IndentPreserve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Region {
    kind: RegionKind,
    /// Just past the opener.
    body_start: usize,
    /// The terminator token, or `None` when the region runs to the end of
    /// the template.
    terminator: Option<Range<usize>>,
}

/// An open construct on the walker's stack.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Frame {
    /// An element with a body, by lower-cased name.
    Element(String),
    /// The attribute list of an opening tag whose `>` is on a later line.
    OpenTag,
    /// A block directive.
    Directive,
    /// The body of a `@case` or `@default`.
    Case,
    /// A bracket left open at the end of a line, by its opening byte.
    Bracket(u8),
    /// An attribute value that runs across lines.
    Quote,
}

/// What the `>` of an opening tag does once the attribute list is done.
#[derive(Debug, Clone, PartialEq, Eq)]
enum After {
    Nothing,
    Element { name: String, counts: bool },
    Region(Region),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// A construct opens; the frame is pushed at this offset.
    Open { frame: Frame, counts: bool },
    /// `</name>`.
    CloseElement(String),
    /// The `>` or `/>` of an opening tag. `pops` when the tag's name is on
    /// an earlier line, so an `OpenTag` frame is waiting for it.
    TagEnd { pops: bool, after: After },
    /// `@endif` and every other block closer.
    CloseDirective,
    /// A closing bracket, by the byte that opened it.
    CloseBracket(u8),
    /// The closing quote of a multi-line attribute value.
    CloseQuote,
    /// `@break`, which ends a `@case` body after its own line.
    Break,
    /// `@else`, `@elseif`, `@empty`, and their relatives: closes the
    /// directive like a closer and is followed by an `Open` that reopens
    /// it.
    Else,
    /// `@case` and `@default`: closes the previous case body, if any, and
    /// is followed by an `Open` for the new one.
    Case,
    /// A region begins.
    Region(Region),
    /// A bracket that closed on the line it opened on; nothing to do.
    Dead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Event {
    at: usize,
    kind: Kind,
}

// ── Scanner ─────────────────────────────────────────────────────────

/// One open bracket the scanner has seen and not yet closed.
struct OpenBracket {
    byte: u8,
    /// The `Open` event it emitted, when it counts (a `(` only counts
    /// when it ends its line).
    event: Option<usize>,
}

struct Scanner<'a> {
    src: &'a str,
    bytes: &'a [u8],
    line_starts: Vec<usize>,
    events: Vec<Event>,
    brackets: Vec<OpenBracket>,
    /// The names every `@end…` in the template closes, so a block the
    /// template defines for itself (`@feature` … `@endfeature`, or a
    /// `Blade::if` family) pairs up without being known in advance.
    end_names: HashSet<&'a str>,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            src.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        );
        let mut scanner = Self {
            src,
            bytes: src.as_bytes(),
            line_starts,
            events: Vec::new(),
            brackets: Vec::new(),
            end_names: HashSet::new(),
        };
        scanner.collect_end_names();
        scanner
    }

    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(line) => line - 1,
        }
    }

    /// Whether only spaces, tabs, and a carriage return follow `from` on
    /// its line.
    fn rest_of_line_blank(&self, from: usize) -> bool {
        self.bytes[from..]
            .iter()
            .take_while(|b| **b != b'\n')
            .all(|b| matches!(b, b' ' | b'\t' | b'\r'))
    }

    fn collect_end_names(&mut self) {
        let mut i = 0;
        while let Some(at) = self.bytes[i..].iter().position(|b| *b == b'@') {
            let at = i + at;
            i = at + 1;
            if !boundary_before(self.bytes, at) {
                continue;
            }
            let end = word_end(self.bytes, at + 1);
            let name = &self.src[at + 1..end];
            if let Some(closed) = name.strip_prefix("end")
                && !closed.is_empty()
            {
                self.end_names.insert(closed);
            }
        }
    }

    fn push(&mut self, at: usize, kind: Kind) {
        self.events.push(Event { at, kind });
    }

    fn scan(mut self) -> Vec<Event> {
        let mut i = 0;
        while i < self.bytes.len() {
            i = match self.bytes[i] {
                b'{' if self.bytes[i..].starts_with(b"{{--") => self.scan_comment(i),
                b'{' if is_echo_start(self.bytes, i) => self.scan_echo(i, self.bytes.len()),
                b'@' => self.scan_directive(i),
                b'<' => self.scan_markup(i),
                b'\'' | b'"' if !self.brackets.is_empty() => {
                    self.skip_string(i, self.bytes.len()).unwrap_or(i + 1)
                }
                b'(' | b'[' | b'{' => self.open_bracket(i),
                b')' | b']' | b'}' => self.close_bracket(i),
                _ => i + 1,
            };
        }
        self.events
    }

    // ── Brackets and strings ────────────────────────────────────────

    fn open_bracket(&mut self, at: usize) -> usize {
        let byte = self.bytes[at];
        // A parenthesis is call syntax as often as it is a block, and its
        // contents are continuation lines unless the author broke the
        // line right after it.
        let counts = byte != b'(' || self.rest_of_line_blank(at + 1);
        let event = counts.then(|| {
            self.push(
                at,
                Kind::Open {
                    frame: Frame::Bracket(byte),
                    counts: true,
                },
            );
            self.events.len() - 1
        });
        self.brackets.push(OpenBracket { byte, event });
        at + 1
    }

    fn close_bracket(&mut self, at: usize) -> usize {
        let opener = match self.bytes[at] {
            b')' => b'(',
            b']' => b'[',
            _ => b'{',
        };
        let Some(index) = self.brackets.iter().rposition(|b| b.byte == opener) else {
            return at + 1;
        };
        let open = self.brackets.drain(index..).next().unwrap_or(OpenBracket {
            byte: opener,
            event: None,
        });
        if let Some(event) = open.event {
            if self.line_of(self.events[event].at) == self.line_of(at) {
                // Opened and closed on one line: no line is inside it.
                self.events[event].kind = Kind::Dead;
            } else {
                self.push(at, Kind::CloseBracket(opener));
            }
        }
        at + 1
    }

    /// Skip the string literal opening at `at`, when it closes on the
    /// same line before `limit`; an apostrophe in prose closes nothing.
    fn skip_string(&self, at: usize, limit: usize) -> Option<usize> {
        let quote = self.bytes[at];
        let mut i = at + 1;
        while i < limit {
            match self.bytes[i] {
                b'\n' => return None,
                b'\\' => i += 2,
                b if b == quote => return Some(i + 1),
                _ => i += 1,
            }
        }
        None
    }

    /// Scan `start..end` as code: strings, comments, echoes, and brackets,
    /// with nothing else special.
    fn scan_code(&mut self, start: usize, end: usize) {
        let mut i = start;
        while i < end {
            i = match self.bytes[i] {
                b'{' if self.bytes[i..end].starts_with(b"{{--") => {
                    find(self.bytes, i + 4, end, b"--}}").map_or(end, |at| at + 4)
                }
                b'{' if is_echo_start(self.bytes, i) => self.scan_echo(i, end),
                b'\'' | b'"' => self.skip_string(i, end).unwrap_or(i + 1),
                // A bracket written in a comment closes nothing.
                b'/' | b'#' => skip_php_comment(self.bytes, i).map_or(i + 1, |past| past.min(end)),
                b'(' | b'[' | b'{' => self.open_bracket(i),
                b')' | b']' | b'}' => self.close_bracket(i),
                _ => i + 1,
            };
        }
    }

    // ── Echoes and comments ─────────────────────────────────────────

    /// Scan an echo's expression as code, skipping the delimiters so their
    /// braces count for nothing. An unterminated echo is two literal
    /// braces.
    fn scan_echo(&mut self, at: usize, limit: usize) -> usize {
        let (open, close) = echo_delimiters(self.bytes, at);
        let Some(end) = find(self.bytes, at + open.len(), limit, close.as_bytes()) else {
            return at + open.len();
        };
        self.scan_code(at + open.len(), end);
        end + close.len()
    }

    /// The extent of the echo at `at`, without scanning it.
    fn skip_echo(&self, at: usize) -> usize {
        let (open, close) = echo_delimiters(self.bytes, at);
        find(
            self.bytes,
            at + open.len(),
            self.bytes.len(),
            close.as_bytes(),
        )
        .map_or(at + open.len(), |end| end + close.len())
    }

    fn scan_comment(&mut self, at: usize) -> usize {
        let Some(close) = find(self.bytes, at + 4, self.bytes.len(), b"--}}") else {
            self.region(at, RegionKind::IndentPreserve, at + 4, None);
            return self.bytes.len();
        };
        let end = close + 4;
        let inner = self.src[at + 4..close].trim();
        if inner == DISABLE_MARKER {
            let terminator = self.find_marker_comment(end, ENABLE_MARKER);
            let resume = terminator.as_ref().map_or(self.bytes.len(), |t| t.end);
            self.region(at, RegionKind::Preserve, end, terminator);
            return resume;
        }
        self.region(at, RegionKind::IndentPreserve, at + 4, Some(close..end));
        end
    }

    /// The next `{{-- marker --}}` comment at or after `from`.
    fn find_marker_comment(&self, from: usize, marker: &str) -> Option<Range<usize>> {
        let mut i = from;
        while let Some(start) = find(self.bytes, i, self.bytes.len(), b"{{--") {
            let close = find(self.bytes, start + 4, self.bytes.len(), b"--}}")?;
            if self.src[start + 4..close].trim() == marker {
                return Some(start..close + 4);
            }
            i = close + 4;
        }
        None
    }

    /// Record a region, unless it opens and closes on one line, in which
    /// case no line is inside it and there is nothing to do.
    fn region(
        &mut self,
        at: usize,
        kind: RegionKind,
        body_start: usize,
        terminator: Option<Range<usize>>,
    ) {
        if let Some(terminator) = &terminator
            && self.line_of(terminator.start) == self.line_of(at)
        {
            return;
        }
        self.push(
            at,
            Kind::Region(Region {
                kind,
                body_start,
                terminator,
            }),
        );
    }

    // ── Directives ──────────────────────────────────────────────────

    /// The next `@name` directive at or after `from`, honouring Blade's
    /// word-boundary rule.
    fn find_directive(&self, from: usize, name: &str) -> Option<Range<usize>> {
        let mut i = from;
        while let Some(at) = self.bytes[i..].iter().position(|b| *b == b'@') {
            let at = i + at;
            i = at + 1;
            if boundary_before(self.bytes, at)
                && self.src[at + 1..].starts_with(name)
                && word_end(self.bytes, at + 1) == at + 1 + name.len()
            {
                return Some(at..at + 1 + name.len());
            }
        }
        None
    }

    fn scan_directive(&mut self, at: usize) -> usize {
        if !boundary_before(self.bytes, at) {
            return at + 1;
        }
        match self.bytes.get(at + 1) {
            // `@@if` is the escape for a literal `@if`.
            Some(b'@') => return at + 2,
            // `@{{ … }}` is a literal echo, whose braces are text.
            Some(b'{') if is_echo_start(self.bytes, at + 1) => return self.skip_echo(at + 1),
            _ => {}
        }
        let name_end = word_end(self.bytes, at + 1);
        if name_end == at + 1 {
            return at + 1;
        }
        let name = &self.src[at + 1..name_end];
        // `@click="…"` is a JavaScript framework's binding; the caller
        // reads it as an attribute.
        if self.bytes.get(name_end) == Some(&b'=') {
            return name_end;
        }

        // Blade allows spaces and tabs, but no newline, between a
        // directive's name and its argument list.
        let mut open = name_end;
        while matches!(self.bytes.get(open), Some(b' ' | b'\t')) {
            open += 1;
        }
        let args = (self.bytes.get(open) == Some(&b'('))
            .then(|| matching_paren(self.bytes, open).map(|close| open..close + 1))
            .flatten();
        let end = args.as_ref().map_or(name_end, |args| args.end);

        match name {
            "php" if args.is_none() => {
                return self.scan_inert_block(at, name_end, "endphp", RegionKind::IndentPreserve);
            }
            "verbatim" => {
                return self.scan_inert_block(at, name_end, "endverbatim", RegionKind::Preserve);
            }
            "php" => return end,
            "endphp" | "endverbatim" => return end,
            _ => {}
        }

        let block = BLOCKS.iter().find(|block| block.opener == name);
        let closes = BLOCKS.iter().any(|block| {
            block.closers.contains(&name) && block.opener != "php" && block.opener != "verbatim"
        });
        let opens_generic = |scanner: &Self| {
            scanner.end_names.contains(name)
                || name
                    .strip_prefix("unless")
                    .is_some_and(|rest| scanner.end_names.contains(rest))
        };

        enum Role {
            Open,
            Close,
            Else,
            Case,
            Break,
            Plain,
        }
        let role = match block {
            Some(block) if opens_block(block, self.src, args.as_ref()) => Role::Open,
            // `@empty` without arguments is `@forelse`'s separator.
            Some(_) if name == "empty" && args.is_none() => Role::Else,
            Some(_) => Role::Plain,
            None if closes => Role::Close,
            None if name.starts_with("else") => Role::Else,
            None if name == "case" || name == "default" => Role::Case,
            None if name == "break" => Role::Break,
            None if name.len() > 3 && name.starts_with("end") => Role::Close,
            None if opens_generic(self) => Role::Open,
            None => Role::Plain,
        };

        match role {
            Role::Close => self.push(at, Kind::CloseDirective),
            Role::Else => self.push(at, Kind::Else),
            Role::Case => self.push(at, Kind::Case),
            Role::Break => self.push(at, Kind::Break),
            Role::Open | Role::Plain => {}
        }
        if let Some(args) = &args {
            self.scan_code(args.start, args.end);
        }
        match role {
            Role::Open | Role::Else => self.push(
                end,
                Kind::Open {
                    frame: Frame::Directive,
                    counts: true,
                },
            ),
            Role::Case => self.push(
                end,
                Kind::Open {
                    frame: Frame::Case,
                    counts: true,
                },
            ),
            _ => {}
        }
        end
    }

    /// A `@php` or `@verbatim` block: nothing inside is a directive, a
    /// tag, or a bracket to the formatter.
    fn scan_inert_block(
        &mut self,
        at: usize,
        name_end: usize,
        closer: &str,
        kind: RegionKind,
    ) -> usize {
        match self.find_directive(name_end, closer) {
            Some(terminator) => {
                let end = terminator.end;
                self.region(at, kind, name_end, Some(terminator));
                end
            }
            None => {
                self.region(at, kind, name_end, None);
                self.bytes.len()
            }
        }
    }

    // ── Markup ──────────────────────────────────────────────────────

    fn scan_markup(&mut self, at: usize) -> usize {
        let rest = &self.bytes[at..];
        if rest.starts_with(b"<!--") {
            return match find(self.bytes, at + 4, self.bytes.len(), b"-->") {
                Some(close) => {
                    self.region(
                        at,
                        RegionKind::IndentPreserve,
                        at + 4,
                        Some(close..close + 3),
                    );
                    close + 3
                }
                None => {
                    self.region(at, RegionKind::IndentPreserve, at + 4, None);
                    self.bytes.len()
                }
            };
        }
        if rest.starts_with(b"<?") {
            return match find(self.bytes, at + 2, self.bytes.len(), b"?>") {
                Some(close) => {
                    self.region(
                        at,
                        RegionKind::IndentPreserve,
                        at + 2,
                        Some(close..close + 2),
                    );
                    close + 2
                }
                None => {
                    self.region(at, RegionKind::IndentPreserve, at + 2, None);
                    self.bytes.len()
                }
            };
        }
        if rest.starts_with(b"<!") {
            return find_byte(self.bytes, at, b'>').map_or(self.bytes.len(), |gt| gt + 1);
        }
        if rest.starts_with(b"</") {
            let Some((name, name_end)) = tag_name(self.src, at + 2) else {
                return at + 1;
            };
            let end = find_byte(self.bytes, name_end, b'>').map_or(self.bytes.len(), |gt| gt + 1);
            self.push(at, Kind::CloseElement(name));
            return end;
        }
        match tag_name(self.src, at + 1) {
            Some((name, name_end)) => self.scan_tag(at, name, name_end),
            None => at + 1,
        }
    }

    fn scan_tag(&mut self, lt: usize, name: String, name_end: usize) -> usize {
        let tag_line = self.line_of(lt);
        let open_tag = self.events.len();
        self.push(
            name_end,
            Kind::Open {
                frame: Frame::OpenTag,
                counts: true,
            },
        );

        let mut i = name_end;
        let (gt, self_closing) = loop {
            while i < self.bytes.len() && self.bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let Some(&byte) = self.bytes.get(i) else {
                // An unterminated tag: its attributes run to the end.
                return self.bytes.len();
            };
            i = match byte {
                b'>' => break (i, false),
                b'/' if self.bytes.get(i + 1) == Some(&b'>') => break (i, true),
                b'/' => i + 1,
                b'{' if self.bytes[i..].starts_with(b"{{--") => self.scan_comment(i),
                b'{' if is_echo_start(self.bytes, i) => self.scan_echo(i, self.bytes.len()),
                b'@' => self.scan_directive(i),
                b'\'' | b'"' => self.scan_attribute_value(i),
                b'(' | b'[' | b'{' => self.open_bracket(i),
                b')' | b']' | b'}' => self.close_bracket(i),
                b if is_attr_name_char(b as char) || b == b'$' => self.scan_attribute(i),
                _ => i + 1,
            };
        };

        let end = gt + if self_closing { 2 } else { 1 };
        let pops = self.line_of(gt) != tag_line;
        if !pops {
            self.events[open_tag].kind = Kind::Dead;
        }

        let after = if self_closing || VOID_ELEMENTS.contains(&name.as_str()) {
            After::Nothing
        } else if let Some(kind) = element_region_kind(&name) {
            match find_closing_tag(self.src, end, &name) {
                Some(terminator) => {
                    let resume = terminator.end;
                    let after = if self.line_of(terminator.start) == self.line_of(gt) {
                        After::Nothing
                    } else {
                        After::Region(Region {
                            kind,
                            body_start: end,
                            terminator: Some(terminator),
                        })
                    };
                    self.push(gt, Kind::TagEnd { pops, after });
                    return resume;
                }
                None => After::Region(Region {
                    kind,
                    body_start: end,
                    terminator: None,
                }),
            }
        } else if UNINDENTED_ELEMENTS.contains(&name.as_str()) {
            After::Element {
                name,
                counts: false,
            }
        } else if (INLINE_ELEMENTS.contains(&name.as_str()) || is_component_name(&name))
            && self.text_follows(end)
        {
            After::Nothing
        } else {
            After::Element { name, counts: true }
        };
        let unterminated_region = matches!(&after, After::Region(r) if r.terminator.is_none());
        self.push(gt, Kind::TagEnd { pops, after });
        if unterminated_region {
            self.bytes.len()
        } else {
            end
        }
    }

    /// Whether text, rather than another tag or the end of the line,
    /// follows the opening tag ending at `from`.
    fn text_follows(&self, from: usize) -> bool {
        self.bytes[from..]
            .iter()
            .find(|b| !matches!(b, b' ' | b'\t'))
            .is_some_and(|b| !matches!(b, b'<' | b'\n' | b'\r'))
    }

    /// An attribute, with its value when it has one.
    fn scan_attribute(&mut self, at: usize) -> usize {
        let mut i = at;
        while i < self.bytes.len()
            && (is_attr_name_char(self.bytes[i] as char) || self.bytes[i] == b'$')
        {
            i += 1;
        }
        let mut j = i;
        while j < self.bytes.len() && self.bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if self.bytes.get(j) != Some(&b'=') {
            return i;
        }
        j += 1;
        while j < self.bytes.len() && self.bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        match self.bytes.get(j) {
            Some(b'\'' | b'"') => self.scan_attribute_value(j),
            Some(_) => {
                // An unquoted value runs to whitespace or the tag's end.
                while j < self.bytes.len()
                    && !self.bytes[j].is_ascii_whitespace()
                    && self.bytes[j] != b'>'
                {
                    j += 1;
                }
                j
            }
            None => j,
        }
    }

    /// A quoted attribute value. One that runs across lines is a frame:
    /// its lines are indented inside it, and the closing quote returns to
    /// the attribute's level.
    fn scan_attribute_value(&mut self, quote_at: usize) -> usize {
        let quote = self.bytes[quote_at];
        let Some(close) = find_byte(self.bytes, quote_at + 1, quote) else {
            return quote_at + 1;
        };
        if self.line_of(quote_at) == self.line_of(close) {
            self.scan_code(quote_at + 1, close);
            return close + 1;
        }
        // A value whose content starts on the next line is a block, one
        // level in; a value whose content starts on the attribute's own
        // line continues at the attribute's level.
        let block = self.rest_of_line_blank(quote_at + 1);
        self.push(
            quote_at,
            Kind::Open {
                frame: Frame::Quote,
                counts: block,
            },
        );
        self.scan_code(quote_at + 1, close);
        self.push(close, Kind::CloseQuote);
        close + 1
    }
}

/// Whether the byte before `at` lets an `@` there start a directive, which
/// is Blade's own word-boundary rule.
pub(super) fn boundary_before(bytes: &[u8], at: usize) -> bool {
    at == 0 || !(bytes[at - 1] == b'@' || is_word_byte(bytes[at - 1]))
}

/// The end of the `\w+` run starting at `from`.
pub(super) fn word_end(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() && is_word_byte(bytes[i]) {
        i += 1;
    }
    i
}

pub(super) fn is_echo_start(bytes: &[u8], at: usize) -> bool {
    bytes[at..].starts_with(b"{{") || bytes[at..].starts_with(b"{!!")
}

/// The opening and closing delimiters of the echo at `at`.
pub(super) fn echo_delimiters(bytes: &[u8], at: usize) -> (&'static str, &'static str) {
    if bytes[at..].starts_with(b"{{{") {
        ("{{{", "}}}")
    } else if bytes[at..].starts_with(b"{!!") {
        ("{!!", "!!}")
    } else {
        ("{{", "}}")
    }
}

/// The tag name starting at `from`: a static name, or a dynamic
/// `{{ $tag }}` echo.
pub(super) fn tag_name(src: &str, from: usize) -> Option<(String, usize)> {
    let bytes = src.as_bytes();
    if bytes[from..].starts_with(b"{{") {
        let close = find(bytes, from + 2, bytes.len(), b"}}")?;
        return Some((src[from..close + 2].to_ascii_lowercase(), close + 2));
    }
    if !bytes.get(from)?.is_ascii_alphabetic() {
        return None;
    }
    let mut end = from;
    while end < bytes.len() && is_tag_name_char(bytes[end] as char) {
        end += 1;
    }
    Some((src[from..end].to_ascii_lowercase(), end))
}

/// The closing tag `</name>` at or after `from`, case-insensitively.
pub(super) fn find_closing_tag(src: &str, from: usize, name: &str) -> Option<Range<usize>> {
    let bytes = src.as_bytes();
    let mut i = from;
    while let Some(at) = find(bytes, i, bytes.len(), b"</") {
        let name_end = at + 2 + name.len();
        if src
            .get(at + 2..name_end)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            && bytes
                .get(name_end)
                .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>')
        {
            let gt = find_byte(bytes, name_end, b'>').unwrap_or(bytes.len() - 1);
            return Some(at..gt + 1);
        }
        i = at + 2;
    }
    None
}

fn element_region_kind(name: &str) -> Option<RegionKind> {
    if PRESERVED_ELEMENTS.contains(&name) {
        Some(RegionKind::Preserve)
    } else if OPAQUE_ELEMENTS.contains(&name) {
        Some(RegionKind::IndentPreserve)
    } else {
        None
    }
}

/// `<x-alert>`, `<flux:button>`, `<livewire:counter>`, and any custom
/// element: a name with a dash or a colon.
pub(super) fn is_component_name(name: &str) -> bool {
    name.contains('-') || name.contains(':')
}

pub(super) fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

pub(super) fn find(bytes: &[u8], from: usize, limit: usize, needle: &[u8]) -> Option<usize> {
    if from >= limit || needle.len() > limit - from {
        return None;
    }
    bytes[from..limit]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

pub(super) fn find_byte(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes[from..]
        .iter()
        .position(|b| *b == needle)
        .map(|at| from + at)
}

// ── Walker ──────────────────────────────────────────────────────────

/// A frame on the stack, with what it contributes to the depth.
#[derive(Debug)]
struct Open {
    frame: Frame,
    /// The line it was opened on.
    line: usize,
    /// Whether the lines inside it are one level deeper. An `<html>`
    /// element, a mid-line attribute value, and an opening tag whose own
    /// line left a bracket open contribute nothing.
    counts: bool,
    /// For a bracket: it took the level away from an open tag on its own
    /// line, so closing it hands the level back.
    cancelled_tag: bool,
    /// For an open tag: a bracket handed its level back on this line, and
    /// the level returns from the next line on, so the closing bracket
    /// itself aligns with the tag.
    restore_after_line: bool,
}

struct ActiveRegion {
    kind: RegionKind,
    /// The depth the opener was written at.
    depth: usize,
    opener_line: usize,
    terminator: Option<Range<usize>>,
    /// The leading whitespace every non-blank body line shares, which the
    /// shift replaces.
    common_prefix: usize,
}

struct Walker<'a> {
    src: &'a str,
    bytes: &'a [u8],
    options: &'a Options,
    lines: Vec<Range<usize>>,
    out: String,
    stack: Vec<Open>,
    depth: usize,
    /// Elements opened inside a directive block that closed before they
    /// did; their closing tags change nothing.
    forgotten: Vec<String>,
    /// How many `?` lines of a ternary the current continuation is deep.
    ternary: usize,
    region: Option<ActiveRegion>,
}

impl<'a> Walker<'a> {
    fn new(src: &'a str, options: &'a Options) -> Self {
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, byte) in src.bytes().enumerate() {
            if byte == b'\n' {
                lines.push(start..i);
                start = i + 1;
            }
        }
        lines.push(start..src.len());
        Self {
            src,
            bytes: src.as_bytes(),
            options,
            lines,
            out: String::with_capacity(src.len() + src.len() / 8),
            stack: Vec::new(),
            depth: 0,
            forgotten: Vec::new(),
            ternary: 0,
            region: None,
        }
    }

    fn first_non_ws(&self, line: &Range<usize>) -> usize {
        self.bytes[line.clone()]
            .iter()
            .position(|b| !matches!(b, b' ' | b'\t'))
            .map_or(line.end, |at| line.start + at)
    }

    fn is_blank(&self, line: &Range<usize>) -> bool {
        let first = self.first_non_ws(line);
        first == line.end || &self.src[first..line.end] == "\r"
    }

    fn walk(mut self, events: &[Event]) -> String {
        let mut next_event = 0;
        for li in 0..self.lines.len() {
            let line = self.lines[li].clone();
            if li > 0 {
                self.out.push('\n');
            }
            let first = self.first_non_ws(&line);

            if let Some(region) = self.region.take() {
                if li > region.opener_line {
                    let terminator_here = region
                        .terminator
                        .clone()
                        .filter(|t| t.start >= line.start && t.start < line.end);
                    match terminator_here {
                        None => {
                            self.emit_region_line(&region, &line, first);
                            self.region = Some(region);
                            continue;
                        }
                        Some(terminator) => {
                            if terminator.start == first {
                                match region.kind {
                                    RegionKind::Preserve => {
                                        self.out.push_str(&self.src[line.clone()])
                                    }
                                    RegionKind::IndentPreserve => {
                                        self.emit_indented(region.depth, 0, &line, first)
                                    }
                                }
                            } else {
                                self.emit_region_line(&region, &line, first);
                            }
                            // The rest of the terminator's line is ordinary
                            // template, whose structure still counts.
                            while next_event < events.len() && events[next_event].at <= line.end {
                                let event = &events[next_event];
                                next_event += 1;
                                if event.at >= terminator.start {
                                    self.pre(event);
                                    self.post(event, li);
                                }
                            }
                            self.end_of_line();
                            continue;
                        }
                    }
                }
                self.region = Some(region);
            }

            let events_start = next_event;
            while next_event < events.len() && events[next_event].at <= line.end {
                next_event += 1;
            }
            let line_events = &events[events_start..next_event];

            if self.is_blank(&line) {
                self.out.push_str(&self.src[first..line.end]);
                for event in line_events {
                    self.pre(event);
                    self.post(event, li);
                }
                self.end_of_line();
                continue;
            }

            let code_context = matches!(
                self.stack.last().map(|open| &open.frame),
                Some(Frame::Bracket(_) | Frame::Quote)
            );
            let extra = self.continuation(&self.src[first..line.end], code_context);

            let mut rest = line_events;
            let indent = match self.lookahead_indent(li, first, &events[next_event..]) {
                Some(indent) => indent,
                None => match line_events.first() {
                    Some(event) if event.at == first => {
                        let indent = self.pre(event);
                        self.post(event, li);
                        rest = &line_events[1..];
                        indent
                    }
                    _ => self.depth,
                },
            };
            for event in rest {
                self.pre(event);
                self.post(event, li);
            }
            self.emit_indented(indent, extra, &line, first);
            self.end_of_line();
        }
        self.out
    }

    // ── Output ──────────────────────────────────────────────────────

    fn emit_indented(&mut self, level: usize, extra: usize, line: &Range<usize>, from: usize) {
        for _ in 0..level + extra {
            self.out.push_str(&self.options.indent);
        }
        self.emit_content(&self.src[from..line.end]);
    }

    fn emit_content(&mut self, content: &str) {
        if self.options.trim_trailing_whitespace {
            let (body, cr) = match content.strip_suffix('\r') {
                Some(body) => (body, "\r"),
                None => (content, ""),
            };
            self.out.push_str(body.trim_end_matches([' ', '\t']));
            self.out.push_str(cr);
        } else {
            self.out.push_str(content);
        }
    }

    fn emit_region_line(&mut self, region: &ActiveRegion, line: &Range<usize>, first: usize) {
        match region.kind {
            RegionKind::Preserve => self.out.push_str(&self.src[line.clone()]),
            RegionKind::IndentPreserve => {
                if self.is_blank(line) {
                    self.out.push_str(&self.src[first..line.end]);
                } else {
                    let from = line.start + region.common_prefix.min(first - line.start);
                    self.emit_indented(region.depth + 1, 0, line, from);
                }
            }
        }
    }

    fn end_of_line(&mut self) {
        for open in &mut self.stack {
            if open.restore_after_line {
                open.restore_after_line = false;
                open.counts = true;
                self.depth += 1;
            }
        }
    }

    /// Extra levels for a line that continues an expression: each `?` of
    /// a ternary nests one deeper, `:` stays at its `?`, and a `.` that
    /// chains a call or concatenates is one level in.
    fn continuation(&mut self, content: &str, code_context: bool) -> usize {
        if !code_context {
            self.ternary = 0;
            return 0;
        }
        let mut chars = content.chars();
        let first = chars.next();
        let second = chars.next();
        let operator_alone = matches!(second, None | Some(' ' | '\t' | '\r'));
        match first {
            Some('?') if operator_alone => self.ternary += 1,
            Some(':') if operator_alone => {}
            _ => self.ternary = 0,
        }
        if self.ternary > 0 {
            return self.ternary;
        }
        let chains = first == Some('.')
            && second
                .is_none_or(|c| c.is_alphabetic() || matches!(c, ' ' | '\t' | '\r' | '_' | '$'));
        usize::from(chains)
    }

    /// A comment on a line of its own before `@else` or `@case` belongs
    /// to the directive it introduces, and sits at its level.
    fn lookahead_indent(&self, li: usize, first: usize, later: &[Event]) -> Option<usize> {
        let content = self.src[first..self.lines[li].end].trim_end();
        let comment_only = [("{{--", "--}}"), ("<!--", "-->")]
            .iter()
            .any(|(open, close)| {
                content.starts_with(open)
                    && content.ends_with(close)
                    && content.find(close) == Some(content.len() - close.len())
            });
        if !comment_only {
            return None;
        }
        let next_first = self.lines[li + 1..]
            .iter()
            .find(|line| !self.is_blank(line))
            .map(|line| self.first_non_ws(line))?;
        let event = later.iter().find(|event| event.at >= next_first)?;
        if event.at != next_first {
            return None;
        }
        match event.kind {
            Kind::Else => Some(self.peek_close_directive()),
            Kind::Case => Some(self.peek_close_case()),
            _ => None,
        }
    }

    // ── Stack ───────────────────────────────────────────────────────

    fn depth_below(&self, index: usize) -> usize {
        self.stack[..index]
            .iter()
            .filter(|open| open.counts)
            .count()
    }

    fn push(&mut self, frame: Frame, counts: bool, line: usize) {
        let mut open = Open {
            frame,
            line,
            counts,
            cancelled_tag: false,
            restore_after_line: false,
        };
        if matches!(open.frame, Frame::Bracket(_)) {
            // `<div x-data="{` indents the object's lines one level in
            // from the tag, not two: the bracket takes the tag's level.
            if let Some(tag) = self.innermost_open_tag()
                && self.stack[tag].line == line
                && self.stack[tag].counts
            {
                self.stack[tag].counts = false;
                self.depth -= 1;
                open.cancelled_tag = true;
            }
        }
        if open.counts {
            self.depth += 1;
        }
        self.stack.push(open);
    }

    /// The open tag whose attribute list the top of the stack is inside,
    /// if the stack is inside one.
    fn innermost_open_tag(&self) -> Option<usize> {
        self.stack
            .iter()
            .rposition(|open| match open.frame {
                Frame::OpenTag => true,
                Frame::Bracket(_) | Frame::Quote => false,
                _ => true,
            })
            .filter(|index| self.stack[*index].frame == Frame::OpenTag)
    }

    /// Remove `open`, handing a level it took from an open tag back.
    fn drop_frame(&mut self, open: Open) {
        if open.counts {
            self.depth -= 1;
        }
        if open.cancelled_tag
            && let Some(tag) = self.innermost_open_tag()
            && !self.stack[tag].counts
        {
            self.stack[tag].restore_after_line = true;
        }
    }

    /// Pop everything from `index` up. Elements above a directive closer
    /// are remembered so their closing tags change nothing.
    fn pop_from(&mut self, index: usize, remember_elements: bool) {
        while self.stack.len() > index {
            let open = self.stack.pop().expect("length checked");
            if remember_elements
                && self.stack.len() > index
                && let Frame::Element(name) = &open.frame
            {
                self.forgotten.push(name.clone());
            }
            self.drop_frame(open);
        }
    }

    fn close_element(&mut self, name: &str) {
        let Some(index) = self
            .stack
            .iter()
            .rposition(|open| matches!(&open.frame, Frame::Element(n) if n == name))
        else {
            if let Some(at) = self.forgotten.iter().rposition(|n| n == name) {
                self.forgotten.remove(at);
            }
            return;
        };
        // Directive frames opened inside the element outlive it: an
        // `@endif` is written for them later. Anything else left open in
        // there is malformed and goes.
        let above: Vec<Open> = self.stack.drain(index..).collect();
        for (k, open) in above.into_iter().enumerate() {
            if k > 0 && matches!(open.frame, Frame::Directive | Frame::Case) {
                self.stack.push(open);
            } else {
                self.drop_frame(open);
            }
        }
    }

    fn pop_open_tag(&mut self) {
        if let Some(index) = self
            .stack
            .iter()
            .rposition(|open| open.frame == Frame::OpenTag)
        {
            self.pop_from(index, false);
        }
    }

    fn innermost_directive(&self) -> Option<usize> {
        self.stack
            .iter()
            .rposition(|open| open.frame == Frame::Directive)
    }

    fn close_directive(&mut self) {
        if let Some(index) = self.innermost_directive() {
            self.pop_from(index, true);
        }
    }

    fn peek_close_directive(&self) -> usize {
        self.innermost_directive()
            .map_or(self.depth, |index| self.depth_below(index))
    }

    /// The `@case` body the top of the stack is inside, if the innermost
    /// directive is a `@switch` with a case open.
    fn innermost_case(&self) -> Option<usize> {
        self.stack
            .iter()
            .rposition(|open| matches!(open.frame, Frame::Case | Frame::Directive))
            .filter(|index| self.stack[*index].frame == Frame::Case)
    }

    fn close_case(&mut self) {
        if let Some(index) = self.innermost_case() {
            self.pop_from(index, true);
        }
    }

    fn peek_close_case(&self) -> usize {
        self.innermost_case()
            .map_or(self.depth, |index| self.depth_below(index))
    }

    fn close_bracket(&mut self, opener: u8) {
        let mut index = self.stack.len();
        while index > 0 {
            match self.stack[index - 1].frame {
                Frame::Bracket(byte) if byte == opener => {
                    self.pop_from(index - 1, false);
                    return;
                }
                Frame::Bracket(_) => index -= 1,
                _ => return,
            }
        }
    }

    fn close_quote(&mut self) {
        let mut index = self.stack.len();
        while index > 0 {
            match self.stack[index - 1].frame {
                Frame::Quote => {
                    self.pop_from(index - 1, false);
                    return;
                }
                Frame::Bracket(_) => index -= 1,
                _ => return,
            }
        }
    }

    // ── Events ──────────────────────────────────────────────────────

    /// Apply the part of `event` that happens before its line is written,
    /// and return the level the line is written at when the event is the
    /// first thing on it.
    fn pre(&mut self, event: &Event) -> usize {
        match &event.kind {
            Kind::CloseElement(name) => self.close_element(name),
            Kind::TagEnd { pops: true, .. } => self.pop_open_tag(),
            Kind::CloseDirective | Kind::Else => self.close_directive(),
            Kind::CloseBracket(opener) => self.close_bracket(*opener),
            Kind::CloseQuote => self.close_quote(),
            Kind::Case => self.close_case(),
            _ => {}
        }
        self.depth
    }

    /// Apply the part of `event` that happens after its line is written.
    fn post(&mut self, event: &Event, line: usize) {
        match &event.kind {
            Kind::Open { frame, counts } => self.push(frame.clone(), *counts, line),
            Kind::TagEnd { after, .. } => match after {
                After::Nothing => {}
                After::Element { name, counts } => {
                    self.push(Frame::Element(name.clone()), *counts, line)
                }
                After::Region(region) => self.start_region(region, line),
            },
            Kind::Break => {
                if self
                    .stack
                    .last()
                    .is_some_and(|open| open.frame == Frame::Case)
                {
                    let index = self.stack.len() - 1;
                    self.pop_from(index, false);
                }
            }
            Kind::Region(region) => self.start_region(region, line),
            _ => {}
        }
    }

    fn start_region(&mut self, region: &Region, line: usize) {
        let body_end = region
            .terminator
            .as_ref()
            .map_or(self.src.len(), |t| t.start);
        let mut common: Option<&str> = None;
        for candidate in &self.lines[line + 1..] {
            let first = self.first_non_ws(candidate);
            if first >= body_end {
                break;
            }
            if self.is_blank(candidate) {
                continue;
            }
            let leading = &self.src[candidate.start..first];
            common = Some(match common {
                None => leading,
                Some(shared) => {
                    let len = shared
                        .bytes()
                        .zip(leading.bytes())
                        .take_while(|(a, b)| a == b)
                        .count();
                    &shared[..len]
                }
            });
        }
        self.region = Some(ActiveRegion {
            kind: region.kind,
            depth: self.depth,
            opener_line: line,
            terminator: region.terminator.clone(),
            common_prefix: common.map_or(0, str::len),
        });
    }
}

/// Apply the end-of-file options to the reindented template.
fn finish_file(out: &mut String, original: &str, options: &Options) {
    if original.is_empty() {
        return;
    }
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    if options.trim_final_newlines {
        let trimmed = out.trim_end_matches(['\n', '\r']).len();
        if trimmed < out.len() {
            out.truncate(trimmed);
            out.push_str(newline);
        }
    }
    if options.insert_final_newline && !out.ends_with('\n') {
        out.push_str(newline);
    }
}
