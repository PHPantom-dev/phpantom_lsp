//! The first pass over a template: every opener, closer, region, and
//! bracket becomes an [`Event`] at its byte offset, in template order,
//! for the walker to replay against its stack.

use std::collections::HashSet;
use std::ops::Range;

use crate::blade::balance::{BLOCKS, opens_block};
use crate::blade::component_tags::{is_attr_name_char, is_tag_name_char};
use crate::blade::directives::{DirectiveHead, boundary_before, directive_head, word_end};
use crate::blade::signature::{echo_delimiters, is_echo_start};
use crate::text_position::LineIndex;
use crate::text_scan::{find, find_byte, skip_php_comment};

use super::elements::{
    INLINE_ELEMENTS, UNINDENTED_ELEMENTS, VOID_ELEMENTS, element_region_kind, is_component_name,
};
use super::{After, DISABLE_MARKER, ENABLE_MARKER, Event, Frame, Kind, Region, RegionKind};

/// One open bracket the scanner has seen and not yet closed.
struct OpenBracket {
    byte: u8,
    /// The `Open` event it emitted, when it counts (a `(` only counts
    /// when it ends its line).
    event: Option<usize>,
}

pub(super) struct Scanner<'a> {
    src: &'a str,
    bytes: &'a [u8],
    lines: LineIndex<'a>,
    events: Vec<Event>,
    brackets: Vec<OpenBracket>,
    /// The names every `@end…` in the template closes, so a block the
    /// template defines for itself (`@feature` … `@endfeature`, or a
    /// `Blade::if` family) pairs up without being known in advance.
    end_names: HashSet<&'a str>,
}

impl<'a> Scanner<'a> {
    pub(super) fn new(src: &'a str) -> Self {
        let mut scanner = Self {
            src,
            bytes: src.as_bytes(),
            lines: LineIndex::new(src),
            events: Vec::new(),
            brackets: Vec::new(),
            end_names: HashSet::new(),
        };
        scanner.collect_end_names();
        scanner
    }

    fn line_of(&self, offset: usize) -> usize {
        self.lines.line_of(offset)
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

    pub(super) fn scan(mut self) -> Vec<Event> {
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
        find_marker_comment(self.src, self.bytes, from, self.bytes.len(), marker)
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
        find_directive(self.src, self.bytes, from, self.bytes.len(), name)
    }

    fn scan_directive(&mut self, at: usize) -> usize {
        let (name, name_end, args) =
            match directive_head(self.src, self.bytes, at, self.bytes.len()) {
                DirectiveHead::None(end)
                | DirectiveHead::Escaped(end)
                | DirectiveHead::LiteralEcho(end) => {
                    return end;
                }
                DirectiveHead::Named {
                    name,
                    name_end,
                    args,
                    ..
                } => (name, name_end, args),
            };
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
        match attribute_head(self.bytes, at, self.bytes.len()).1 {
            AttributeValue::None(end) | AttributeValue::Unquoted(end) => end,
            AttributeValue::Quoted(quote_at) => self.scan_attribute_value(quote_at),
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

/// The tag name starting at `from`: a static name, or a dynamic
/// `{{ $tag }}` echo.
pub(crate) fn tag_name(src: &str, from: usize) -> Option<(String, usize)> {
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
pub(crate) fn find_closing_tag(src: &str, from: usize, name: &str) -> Option<Range<usize>> {
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

/// The next `@name` directive at or after `from`, honouring Blade's
/// word-boundary rule. A delimiter written inside a string literal counts,
/// because Blade's own scan for one is a non-greedy regex that knows
/// nothing about PHP: this has to agree with the reindenter and with the
/// compiler about where a block ends.
pub(crate) fn find_directive(
    src: &str,
    bytes: &[u8],
    from: usize,
    limit: usize,
    name: &str,
) -> Option<Range<usize>> {
    let mut i = from;
    while let Some(at) = find(bytes, i, limit, b"@") {
        i = at + 1;
        if boundary_before(bytes, at)
            && src[at + 1..].starts_with(name)
            && word_end(bytes, at + 1) == at + 1 + name.len()
        {
            return Some(at..at + 1 + name.len());
        }
    }
    None
}

/// The next `{{-- marker --}}` comment at or after `from`, bounded by
/// `limit`.
pub(crate) fn find_marker_comment(
    src: &str,
    bytes: &[u8],
    from: usize,
    limit: usize,
    marker: &str,
) -> Option<Range<usize>> {
    let mut i = from;
    while let Some(start) = find(bytes, i, limit, b"{{--") {
        let close = find(bytes, start + 4, limit, b"--}}")?;
        if src[start + 4..close].trim() == marker {
            return Some(start..close + 4);
        }
        i = close + 4;
    }
    None
}

/// Where an attribute's value starts, once its name and any `=` have
/// been lexed.
pub(crate) enum AttributeValue {
    /// A boolean attribute, or one whose `=` is not followed by a value:
    /// the attribute ends at this offset.
    None(usize),
    /// A quoted value opens at this offset (the quote byte itself).
    Quoted(usize),
    /// An unquoted value, ending at this offset.
    Unquoted(usize),
}

/// Lex one attribute's name and, when present, where its value begins.
/// `limit` bounds the scan.
pub(crate) fn attribute_head(
    bytes: &[u8],
    at: usize,
    limit: usize,
) -> (Range<usize>, AttributeValue) {
    let mut i = at;
    while i < limit && (is_attr_name_char(bytes[i] as char) || bytes[i] == b'$') {
        i += 1;
    }
    let name = at..i;
    let mut j = i;
    while j < limit && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if bytes.get(j) != Some(&b'=') {
        return (name, AttributeValue::None(i));
    }
    j += 1;
    while j < limit && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    match bytes.get(j) {
        Some(b'\'' | b'"') => (name, AttributeValue::Quoted(j)),
        Some(_) => {
            // An unquoted value runs to whitespace or the tag's end.
            let mut k = j;
            while k < limit && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                k += 1;
            }
            (name, AttributeValue::Unquoted(k))
        }
        None => (name, AttributeValue::None(j)),
    }
}
