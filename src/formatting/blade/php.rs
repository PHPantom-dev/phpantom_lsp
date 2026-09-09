//! Formatting the PHP a Blade template carries.
//!
//! The reindenter changes leading whitespace only, so the PHP inside a
//! template keeps whatever spacing its author typed. This pass rewrites
//! each PHP fragment through the embedded mago formatter, on an isolated
//! snippet rather than on the virtual PHP the preprocessor lowers a
//! template to: a `@php` body and a `<?php` island are a statement list,
//! an echo, a `<?=` island, and a directive's argument list a single
//! expression.
//!
//! Reindenting happens afterwards, on the rewritten template, so a
//! fragment that gains or loses lines needs no handling here: the
//! reindenter shifts a `@php` body to its block's level the same way it
//! shifts any other body.
//!
//! It also happens before. A snippet is formatted on its own, so mago
//! fills it to the print width as though it started at column 0, and a
//! `@php` block eight levels deep would come back filled to 120 columns
//! and then be pushed past the end of the line. The pass therefore runs
//! between two reindents: the first settles every block's column, the
//! pass subtracts that column from the print width it formats the block
//! with, and the second lays out what the pass rewrote. Running the
//! reindenter first is also what makes the result stable, since the
//! columns the pass reads are the ones it will read again next time.
//!
//! The same subtraction applies to an echo or an argument list that
//! spans lines, whose first line continues the line it sits on and
//! whose rest the reindenter writes one level in from it.
//!
//! Line structure stays the author's. A fragment written on one line
//! comes back on one line or not at all, and one written across lines
//! comes back across lines or not at all: the pass owns the spacing
//! inside a fragment, not whether the fragment takes one line or
//! several. Where a fragment stays broken the breaks within it are
//! mago's, which by default keeps an array the author broke broken.
//!
//! Spacing that is Blade's rather than PHP's is part of the same pass,
//! because it is the same scan: `@if(` becomes `@if (`, `{{$x}}` becomes
//! `{{ $x }}`, and a self-closing tag gets one space before its `/>`.
//!
//! Nothing runs inside `@verbatim`, a Blade or HTML comment, a
//! `blade-formatter-disable` region, `<script>`, `<style>`, `<pre>`,
//! `<textarea>`, or an Alpine or Livewire attribute value, which is
//! JavaScript. A fragment mago cannot parse is left exactly as written,
//! and so is one whose result would not fit back into the lines it was
//! written on.

use std::borrow::Cow;
use std::ops::Range;

use mago_formatter::Formatter;
use mago_formatter::settings::FormatSettings;
use mago_php_version::PHPVersion;

use crate::atom::bytes_to_str;
use crate::blade::component_tags::is_attr_name_char;
use crate::blade::signature::matching_paren;

use super::reindent::{
    DISABLE_MARKER, ENABLE_MARKER, OPAQUE_ELEMENTS, PRESERVED_ELEMENTS, boundary_before,
    echo_delimiters, find, find_byte, find_closing_tag, is_component_name, is_echo_start, tag_name,
    word_end,
};

/// What the embedded formatter needs to format one fragment: the
/// project's PHP version and the settings the built-in PHP strategy
/// resolved, `mago.toml` included.
pub(super) struct PhpSettings {
    pub version: PHPVersion,
    pub settings: FormatSettings,
}

/// Directives Blade compiles to a PHP control structure, whose keyword
/// takes a space before its condition. Every other directive compiles to
/// a call and is written like one, which is how Laravel's own
/// documentation spells both: `@if (…)` and `@foreach (…)`, but
/// `@isset(…)`, `@switch(…)`, and `@include(…)`.
const SPACED_DIRECTIVES: &[&str] = &[
    "if", "elseif", "unless", "for", "foreach", "forelse", "while",
];

/// The synthetic call an expression or argument list is formatted inside.
/// It has to end with its `(` so the result can be unwrapped again.
const CALL_WRAPPER: &str = "__phpantom_format(";

/// Rewrite every PHP fragment in `content`, and return the template with
/// the results spliced back in.
///
/// `content` has to be reindented already: a fragment's column is read
/// off it, and the caller reindents the result again.
pub(super) fn format_embedded(content: &str, php: &PhpSettings, indent: &str) -> String {
    let arena = mago_allocator::LocalArena::new();
    let mut scanner = Scanner {
        src: content,
        bytes: content.as_bytes(),
        arena: &arena,
        php,
        indent: display_width(indent, php.settings.tab_width),
        edits: Vec::new(),
    };
    scanner.walk_template();
    splice(content, &scanner.edits)
}

/// How many columns `whitespace` occupies, counting a tab as a whole
/// indentation level.
fn display_width(whitespace: &str, tab_width: usize) -> usize {
    whitespace
        .chars()
        .map(|c| if c == '\t' { tab_width } else { 1 })
        .sum()
}

/// One fragment's source range and what replaces it. The scanner walks
/// forward, so edits arrive sorted and cannot overlap.
struct Edit {
    span: Range<usize>,
    text: String,
}

fn splice(src: &str, edits: &[Edit]) -> String {
    if edits.is_empty() {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    for edit in edits {
        out.push_str(&src[at..edit.span.start]);
        out.push_str(&edit.text);
        at = edit.span.end;
    }
    out.push_str(&src[at..]);
    out
}

struct Scanner<'a> {
    src: &'a str,
    bytes: &'a [u8],
    /// One arena for the whole template: a `Formatter` is four fields
    /// and is built per fragment, but the allocations behind it are not.
    arena: &'a mago_allocator::LocalArena,
    php: &'a PhpSettings,
    /// One level of the reindenter's indentation, in columns.
    indent: usize,
    edits: Vec<Edit>,
}

impl Scanner<'_> {
    // ── Walking ─────────────────────────────────────────────────────

    /// Template markup: echoes, directives, and tags.
    fn walk_template(&mut self) {
        let end = self.bytes.len();
        let mut i = 0;
        while i < end {
            i = match self.bytes[i] {
                b'{' if self.bytes[i..].starts_with(b"{{--") => self.comment(i, end),
                b'{' if is_echo_start(self.bytes, i) => self.echo(i, end),
                b'@' => self.directive(i, end),
                b'<' => self.markup(i),
                _ => i + 1,
            };
        }
    }

    /// A stretch of template text rather than markup — an attribute value
    /// — where a `<` is a character and only echoes and directives carry
    /// PHP.
    fn walk_text(&mut self, start: usize, end: usize) {
        let mut i = start;
        while i < end {
            i = match self.bytes[i] {
                b'{' if self.bytes[i..end].starts_with(b"{{--") => self.comment(i, end),
                b'{' if is_echo_start(self.bytes, i) => self.echo(i, end),
                b'@' => self.directive(i, end),
                _ => i + 1,
            };
        }
    }

    // ── Echoes and comments ─────────────────────────────────────────

    /// `{{ … }}`, `{!! … !!}`, or `{{{ … }}}`: one expression, and the one
    /// space each side that every Blade formatter writes.
    fn echo(&mut self, at: usize, limit: usize) -> usize {
        let (open, close) = echo_delimiters(self.bytes, at);
        let body_start = at + open.len();
        let Some(body_end) = find(self.bytes, body_start, limit, close.as_bytes()) else {
            return body_start;
        };
        let end = body_end + close.len();
        if let Some(expression) = self.expression(body_start..body_end) {
            self.replace(at..end, format!("{open} {expression} {close}"));
        }
        end
    }

    /// `<?= … ?>`: one expression, like an echo. The `;` it may end with
    /// is optional, so it stays or goes as the author wrote it.
    fn short_echo(&mut self, body: Range<usize>) {
        let text = self.src[body.clone()].trim_end();
        let (expression_end, semicolon) = match text.strip_suffix(';') {
            Some(head) => (body.start + head.len(), ";"),
            None => (body.start + text.len(), ""),
        };
        if let Some(expression) = self.expression(body.start..expression_end) {
            self.replace(body, format!(" {expression}{semicolon} "));
        }
    }

    /// A `{{-- … --}}` comment, whose text is not PHP. The one that turns
    /// formatting off takes everything up to its matching `enable` with
    /// it.
    fn comment(&mut self, at: usize, limit: usize) -> usize {
        let Some(close) = find(self.bytes, at + 4, limit, b"--}}") else {
            return limit;
        };
        let end = close + 4;
        if self.src[at + 4..close].trim() == DISABLE_MARKER {
            return self.find_marker(end, limit, ENABLE_MARKER).unwrap_or(limit);
        }
        end
    }

    /// The end of the next `{{-- marker --}}` comment at or after `from`.
    fn find_marker(&self, from: usize, limit: usize, marker: &str) -> Option<usize> {
        let mut i = from;
        while let Some(start) = find(self.bytes, i, limit, b"{{--") {
            let close = find(self.bytes, start + 4, limit, b"--}}")?;
            if self.src[start + 4..close].trim() == marker {
                return Some(close + 4);
            }
            i = close + 4;
        }
        None
    }

    // ── Directives ──────────────────────────────────────────────────

    fn directive(&mut self, at: usize, limit: usize) -> usize {
        if !boundary_before(self.bytes, at) {
            return at + 1;
        }
        match self.bytes.get(at + 1) {
            // `@@if` is the escape for a literal `@if`.
            Some(b'@') => return at + 2,
            // `@{{ … }}` is a literal echo, whose text a frontend
            // framework renders rather than PHP.
            Some(b'{') if is_echo_start(self.bytes, at + 1) => {
                let (open, close) = echo_delimiters(self.bytes, at + 1);
                let body_start = at + 1 + open.len();
                return find(self.bytes, body_start, limit, close.as_bytes())
                    .map_or(body_start, |end| end + close.len());
            }
            _ => {}
        }
        let name_end = word_end(self.bytes, at + 1);
        if name_end == at + 1 {
            return at + 1;
        }
        let name = &self.src[at + 1..name_end];
        // `@click="…"` is a JavaScript framework's binding, not a
        // directive; the caller reads it as an attribute.
        if self.bytes.get(name_end) == Some(&b'=') {
            return name_end;
        }

        // Blade allows spaces and tabs, but no newline, between a
        // directive's name and its argument list.
        let mut open = name_end;
        while matches!(self.bytes.get(open), Some(b' ' | b'\t')) {
            open += 1;
        }
        let has_args = self.bytes.get(open) == Some(&b'(');
        match name {
            "verbatim" => {
                return self
                    .find_directive(name_end, limit, "endverbatim")
                    .map_or(limit, |terminator| terminator.end);
            }
            "php" if !has_args => return self.php_block(at, name_end, limit),
            _ => {}
        }
        if !has_args {
            return name_end;
        }
        let Some(close) = matching_paren(self.bytes, open).filter(|close| *close < limit) else {
            return open + 1;
        };
        self.replace(
            name_end..open,
            if SPACED_DIRECTIVES.contains(&name) {
                " ".to_string()
            } else {
                String::new()
            },
        );
        if let Some(formatted) = self.arguments(name, open + 1..close) {
            self.replace(open + 1..close, formatted);
        }
        close + 1
    }

    /// A `@php … @endphp` block, whose body is a statement list. `at` is
    /// the `@`, whose column the body is laid out against.
    fn php_block(&mut self, at: usize, name_end: usize, limit: usize) -> usize {
        match self.find_directive(name_end, limit, "endphp") {
            Some(terminator) => {
                self.statement_block(at, name_end..terminator.start);
                terminator.end
            }
            None => limit,
        }
    }

    /// The next `@name` at or after `from`, honouring Blade's word-boundary
    /// rule. A delimiter written inside a string literal counts, because
    /// Blade's own scan for one is a non-greedy regex that knows nothing
    /// about PHP: this has to agree with the reindenter and with the
    /// compiler about where a block ends.
    fn find_directive(&self, from: usize, limit: usize, name: &str) -> Option<Range<usize>> {
        let mut i = from;
        while let Some(at) = find(self.bytes, i, limit, b"@") {
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

    // ── Markup ──────────────────────────────────────────────────────

    fn markup(&mut self, at: usize) -> usize {
        let end = self.bytes.len();
        let rest = &self.bytes[at..];
        if rest.starts_with(b"<!--") {
            return find(self.bytes, at + 4, end, b"-->").map_or(end, |close| close + 3);
        }
        // `<?xml` is never a PHP tag, whatever `short_open_tag` says.
        if rest.starts_with(b"<?xml") {
            return at + 5;
        }
        if rest.starts_with(b"<?php") {
            return match find(self.bytes, at + 5, end, b"?>") {
                Some(close) => {
                    self.statement_block(at, at + 5..close);
                    close + 2
                }
                None => end,
            };
        }
        if rest.starts_with(b"<?=") {
            return match find(self.bytes, at + 3, end, b"?>") {
                Some(close) => {
                    self.short_echo(at + 3..close);
                    close + 2
                }
                None => end,
            };
        }
        // A short `<?` tag is skipped rather than formatted: whether it
        // opens PHP at all depends on `short_open_tag`, and what follows
        // it is a statement list only when it does.
        if rest.starts_with(b"<?") {
            return find(self.bytes, at + 2, end, b"?>").map_or(end, |close| close + 2);
        }
        if rest.starts_with(b"<!") {
            return find_byte(self.bytes, at, b'>').map_or(end, |gt| gt + 1);
        }
        if rest.starts_with(b"</") {
            return find_byte(self.bytes, at + 2, b'>').map_or(end, |gt| gt + 1);
        }
        match tag_name(self.src, at + 1) {
            Some((name, name_end)) => self.tag(&name, name_end),
            None => at + 1,
        }
    }

    /// An opening tag's attribute list, up to its `>` or `/>`. An element
    /// whose body is another language is skipped whole.
    fn tag(&mut self, name: &str, name_end: usize) -> usize {
        let end = self.bytes.len();
        let component = is_component_name(name);
        let mut i = name_end;
        let (gt, self_closing) = loop {
            let gap = i;
            while i < end && self.bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let Some(&byte) = self.bytes.get(i) else {
                // An unterminated tag: its attributes run to the end.
                return end;
            };
            i = match byte {
                b'>' => break (i, false),
                b'/' if self.bytes.get(i + 1) == Some(&b'>') => {
                    self.space_before_close(gap, i);
                    break (i, true);
                }
                b'/' => i + 1,
                b'{' if self.bytes[i..].starts_with(b"{{--") => self.comment(i, end),
                b'{' if is_echo_start(self.bytes, i) => self.echo(i, end),
                b'@' => self.directive(i, end),
                // A quote reached without an attribute name in front of it
                // is one whose name the directive scan consumed, i.e. an
                // `@click="…"` binding: JavaScript.
                b'\'' | b'"' => self.attribute_value(i, false),
                b if is_attr_name_char(b as char) || b == b'$' => self.attribute(i, component),
                _ => i + 1,
            };
        };
        let tag_end = gt + if self_closing { 2 } else { 1 };
        if !self_closing && (OPAQUE_ELEMENTS.contains(&name) || PRESERVED_ELEMENTS.contains(&name))
        {
            return find_closing_tag(self.src, tag_end, name).map_or(end, |close| close.end);
        }
        tag_end
    }

    /// `<br/>` becomes `<br />`. A gap that spans lines is the tag's own
    /// layout, which the reindenter owns.
    fn space_before_close(&mut self, from: usize, slash: usize) {
        if !self.src[from..slash].contains('\n') {
            self.replace(from..slash, " ".to_string());
        }
    }

    /// One attribute, with its value when it has one.
    fn attribute(&mut self, at: usize, component: bool) -> usize {
        let end = self.bytes.len();
        let mut i = at;
        while i < end && (is_attr_name_char(self.bytes[i] as char) || self.bytes[i] == b'$') {
            i += 1;
        }
        let name = &self.src[at..i];
        let mut j = i;
        while j < end && self.bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if self.bytes.get(j) != Some(&b'=') {
            return i;
        }
        j += 1;
        while j < end && self.bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        match self.bytes.get(j) {
            Some(b'\'' | b'"') => {
                self.attribute_value(j, !is_javascript_attribute(name, component))
            }
            Some(_) => {
                // An unquoted value runs to whitespace or the tag's end.
                while j < end && !self.bytes[j].is_ascii_whitespace() && self.bytes[j] != b'>' {
                    j += 1;
                }
                j
            }
            None => j,
        }
    }

    fn attribute_value(&mut self, quote_at: usize, blade: bool) -> usize {
        let quote = self.bytes[quote_at];
        let Some(close) = find_byte(self.bytes, quote_at + 1, quote) else {
            return quote_at + 1;
        };
        if blade {
            self.walk_text(quote_at + 1, close);
        }
        close + 1
    }

    // ── Formatting ──────────────────────────────────────────────────

    /// Replace a statement list's source with its formatted form: on one
    /// line when the whole block was on one line, and on its own lines
    /// otherwise, where the reindenter shifts it to the block's level.
    ///
    /// `opener` is the offset of the `@php` or `<?php` that starts it,
    /// whose column decides how much of the print width the body has
    /// left once the reindenter has put it back.
    fn statement_block(&mut self, opener: usize, body: Range<usize>) {
        let source = &self.src[body.clone()];
        let Some(formatted) = self.statements(source, self.body_width(opener)) else {
            return;
        };
        if source.contains('\n') {
            self.replace(body, format!("\n{formatted}\n"));
        } else if !formatted.contains('\n') {
            self.replace(body, format!(" {formatted} "));
        }
    }

    /// The print width left for the body of the block opening at
    /// `opener`, which the reindenter writes one level in from the
    /// opener's own column.
    fn body_width(&self, opener: usize) -> usize {
        let line_start = self.src[..opener].rfind('\n').map_or(0, |at| at + 1);
        let column =
            display_width(&self.src[line_start..opener], self.php.settings.tab_width) + self.indent;
        self.php.settings.print_width.saturating_sub(column).max(1)
    }

    /// A statement list, formatted as the body of a PHP file. The closing
    /// tag stands in for the `@endphp` or `?>` that followed it, so a last
    /// statement without its `;` still parses; mago drops it again unless
    /// it was load-bearing.
    fn statements(&self, body: &str, width: usize) -> Option<String> {
        let body = body.trim();
        if body.is_empty() {
            return None;
        }
        let formatted = self.format_php(&format!("<?php\n{body}\n?>"), width)?;
        Some(
            formatted
                .strip_suffix("?>")
                .map_or(formatted.as_str(), str::trim_end)
                .to_string(),
        )
    }

    /// An echo's expression, or `None` when it is empty or does not
    /// parse.
    fn expression(&self, body: Range<usize>) -> Option<String> {
        self.wrapped(CALL_WRAPPER, body, ");")
    }

    /// A directive's argument list. A loop header is not an expression, so
    /// it is formatted inside the statement Blade compiles it to; every
    /// other directive takes what a call takes, one expression included.
    fn arguments(&self, name: &str, args: Range<usize>) -> Option<String> {
        let (open, close) = match name {
            "foreach" | "forelse" => ("foreach (", "): endforeach;"),
            "for" => ("for (", "): endfor;"),
            _ => (CALL_WRAPPER, ");"),
        };
        self.wrapped(open, args, close)
    }

    /// Format the fragment at `body` inside the `open`…`close` wrapper and
    /// hand back what came out between the wrapper's parentheses, when it
    /// still takes the lines the author wrote it on.
    fn wrapped(&self, open: &str, body: Range<usize>, close: &str) -> Option<String> {
        let raw = &self.src[body.clone()];
        let text = raw.trim();
        if text.is_empty() {
            return None;
        }
        let broken = text.contains('\n');
        let width = if broken {
            self.inline_width(body.start + (raw.len() - raw.trim_start().len()))
        } else {
            // The full print width, not the column-adjusted one: a result
            // that wraps is rejected anyway, so narrowing the width would
            // just reject a long fragment that was already that long in
            // the source.
            self.php.settings.print_width
        };
        let source = format!("<?php\n{open}{text}{close}\n");
        let formatted = self.format_php(&source, width)?;
        // mago writes the wrapper back verbatim, so its own `(` is still
        // the one at the end of `open`. Its columns counted against the
        // width too, which only ever wraps a fragment a shade earlier
        // than it had to.
        let rest = formatted.strip_prefix(open)?;
        let inner =
            rest[..matching_paren(formatted.as_bytes(), open.len() - 1)? - open.len()].trim();
        (inner.contains('\n') == broken).then(|| inner.to_string())
    }

    /// The print width left for a fragment written inline at `body`: its
    /// first line continues the line it sits on, and the reindenter puts
    /// the rest one level in from that line's indentation. One width is
    /// all the formatter takes, so whichever of the two columns leaves
    /// less room decides.
    fn inline_width(&self, body: usize) -> usize {
        let tab_width = self.php.settings.tab_width;
        let line_start = self.src[..body].rfind('\n').map_or(0, |at| at + 1);
        let head = &self.src[line_start..body];
        let leading = &head[..head.len() - head.trim_start().len()];
        let column =
            display_width(head, tab_width).max(display_width(leading, tab_width) + self.indent);
        self.php.settings.print_width.saturating_sub(column).max(1)
    }

    /// Format one snippet to `width` columns and return its body, without
    /// the `<?php` the formatter needs to read it as PHP.
    fn format_php(&self, source: &str, width: usize) -> Option<String> {
        let settings = FormatSettings {
            print_width: width,
            ..self.php.settings
        };
        let formatted = Formatter::new(self.arena, self.php.version, settings)
            .format_code(
                Cow::Borrowed(b"phpantom-blade"),
                Cow::Owned(source.as_bytes().to_vec()),
            )
            .ok()?;
        let formatted = bytes_to_str(formatted);
        Some(
            formatted
                .strip_prefix("<?php")?
                .trim_start_matches('\n')
                .trim_end()
                .to_string(),
        )
    }

    /// Record an edit, unless the fragment is already written that way.
    fn replace(&mut self, span: Range<usize>, text: String) {
        if self.src[span.clone()] != text {
            self.edits.push(Edit { span, text });
        }
    }
}

/// Whether an attribute's value is JavaScript that Alpine or Livewire
/// binds, rather than template text whose `{{ … }}` echoes are PHP. A `:`
/// prefix is Alpine's bind shorthand on a plain element and Blade's
/// expression binding on a component tag, where `::` escapes it back to a
/// literal attribute name.
fn is_javascript_attribute(name: &str, component: bool) -> bool {
    match name.strip_prefix(':') {
        Some(rest) => !component || rest.starts_with(':'),
        None => name.starts_with("x-") || name.starts_with("wire:") || name.starts_with('@'),
    }
}
