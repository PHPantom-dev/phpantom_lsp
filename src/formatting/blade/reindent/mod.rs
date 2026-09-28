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
//! Regions Blade ignores directives in are read the way `crate::
//! blade::balance` reads them, and the block table is the same one the
//! unbalanced-directive diagnostic checks against, so the formatter and
//! the diagnostic cannot disagree about what closes what.

mod elements;
mod scanner;
mod walker;

use std::ops::Range;

pub(super) use elements::{OPAQUE_ELEMENTS, PRESERVED_ELEMENTS, is_component_name};
use scanner::Scanner;
pub(super) use scanner::{
    AttributeValue, attribute_head, find_closing_tag, find_directive, find_marker_comment, tag_name,
};
use walker::{Walker, finish_file};

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
