//! The second pass: replay the scanner's events against a stack of open
//! constructs and write every line out with the leading whitespace its
//! depth calls for.

use std::ops::Range;

use super::{After, Event, Frame, Kind, Options, Region, RegionKind};

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

pub(super) struct Walker<'a> {
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
    pub(super) fn new(src: &'a str, options: &'a Options) -> Self {
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

    pub(super) fn walk(mut self, events: &[Event]) -> String {
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
pub(super) fn finish_file(out: &mut String, original: &str, options: &Options) {
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
