//! The section and stack names a template declares, fills, and checks.
//!
//! `@yield('content')` and `@stack('scripts')` name a hole a layout leaves
//! for someone else to fill, and `@section('content')` / `@push('scripts')`
//! fill one. Neither half knows the other exists: the name is a plain
//! string that Blade only pairs up at render time, in a file the author
//! never opens. A section filled under a name the layout never yields
//! renders nothing at all, and no error says so.
//!
//! This scan reads the raw Blade source and records every such name with
//! the span it occupies, so the two halves can be paired across files. It
//! reads the same directive stream [`super::balance`] does, from the same
//! masked-region scan, because a name written in a comment, a `@verbatim`
//! block, or a `@php` block names nothing.
//!
//! Alongside the names it records what else the template renders
//! (`@extends`, the `@include` family), since a section's other half is
//! usually in one of those files, and whether anything it renders is
//! beyond reading — a dynamic view name, a component tag — so a caller
//! that needs the *complete* set of names in a render tree can tell when
//! it does not have one.

use std::ops::Range;

use super::balance::{Directive, directives, inside};
use super::signature::{inert_regions, mask_regions, split_top_level_args};

/// A byte range of the original Blade source.
pub(crate) type Span = Range<usize>;

/// Which of the two name spaces a directive addresses.
///
/// Blade keeps sections and stacks apart: `@push('content')` does not fill
/// the section `@yield('content')` renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockKind {
    /// `@section` / `@yield` and their helpers.
    Section,
    /// `@push` / `@stack` and their helpers.
    Stack,
}

impl BlockKind {
    /// The word for this name space in a user-facing message.
    pub(crate) fn label(self) -> &'static str {
        match self {
            BlockKind::Section => "section",
            BlockKind::Stack => "stack",
        }
    }
}

/// What a directive does with the name it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockRole {
    /// Renders the name where it stands: `@yield`, `@stack`, and the
    /// `@section` … `@show` that renders its own body.
    Declare,
    /// Supplies content for the name: `@section`, `@push`, `@prepend`.
    Fill,
    /// Asks whether the name was filled: `@hasSection`, `@sectionMissing`,
    /// `@hasStack`.
    Check,
}

/// One section or stack name as it appears in a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockRef {
    /// The name itself, without its quotes.
    pub(crate) name: String,
    pub(crate) kind: BlockKind,
    pub(crate) role: BlockRole,
    /// The name's own bytes, inside the quotes, in the raw Blade source.
    pub(crate) name_span: Span,
    /// The directive that holds the name (`@section`), without its
    /// argument list.
    pub(crate) directive_span: Span,
}

/// A directive whose argument list holds a section or stack name.
pub(crate) struct NamedBlockDirective {
    pub(crate) name: &'static str,
    pub(crate) kind: BlockKind,
    role: BlockRole,
    /// Where the name sits among the directive's arguments.
    argument: NameArgument,
}

/// Which argument of a directive holds the name.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NameArgument {
    /// `@yield('content', 'default')`, `@pushOnce('scripts', $id)`.
    First,
    /// `@pushIf($condition, 'scripts')`. Laravel folds everything before
    /// the last comma into the condition, so the name is the last argument
    /// however many commas the condition itself contains.
    Last,
}

/// Every directive that names a section or a stack.
///
/// One table for the whole feature: the preprocessor picks the marker call
/// it lowers each of these to from it, completion decides what to offer
/// inside one from it, and this scan reads them from it.
pub(crate) const NAMED_BLOCK_DIRECTIVES: &[NamedBlockDirective] = &[
    NamedBlockDirective {
        name: "section",
        kind: BlockKind::Section,
        role: BlockRole::Fill,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "yield",
        kind: BlockKind::Section,
        role: BlockRole::Declare,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "hasSection",
        kind: BlockKind::Section,
        role: BlockRole::Check,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "sectionMissing",
        kind: BlockKind::Section,
        role: BlockRole::Check,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "stack",
        kind: BlockKind::Stack,
        role: BlockRole::Declare,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "push",
        kind: BlockKind::Stack,
        role: BlockRole::Fill,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "pushOnce",
        kind: BlockKind::Stack,
        role: BlockRole::Fill,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "prepend",
        kind: BlockKind::Stack,
        role: BlockRole::Fill,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "prependOnce",
        kind: BlockKind::Stack,
        role: BlockRole::Fill,
        argument: NameArgument::First,
    },
    NamedBlockDirective {
        name: "pushIf",
        kind: BlockKind::Stack,
        role: BlockRole::Fill,
        argument: NameArgument::Last,
    },
    NamedBlockDirective {
        name: "hasStack",
        kind: BlockKind::Stack,
        role: BlockRole::Check,
        argument: NameArgument::First,
    },
];

/// The call the preprocessor lowers a section-naming directive to.
///
/// A marker of its own rather than the generic `blade_directive`: symbol
/// extraction recognises a Laravel string key by the callee it is passed
/// to, and `@class(['a' => $b])` shares that generic one.
pub(crate) const SECTION_MARKER: &str = "blade_section_directive";
/// The same for a stack-naming directive.
pub(crate) const STACK_MARKER: &str = "blade_stack_directive";
/// The same for `@pushIf`, whose stack name is its *last* argument rather
/// than its first.
pub(crate) const PUSH_IF_MARKER: &str = "blade_push_if_directive";

impl NamedBlockDirective {
    /// Whether the directive compiles to a condition rather than a
    /// statement, as `@hasSection` and its cousins do.
    pub(crate) fn opens_condition(&self) -> bool {
        matches!(self.role, BlockRole::Check)
    }

    /// The marker call the preprocessor lowers this directive to.
    pub(crate) fn marker(&self) -> &'static str {
        match (self.kind, self.argument) {
            (_, NameArgument::Last) => PUSH_IF_MARKER,
            (BlockKind::Section, _) => SECTION_MARKER,
            (BlockKind::Stack, _) => STACK_MARKER,
        }
    }
}

/// The entry for `directive`, when it names a section or a stack.
pub(crate) fn named_block_directive(directive: &str) -> Option<&'static NamedBlockDirective> {
    NAMED_BLOCK_DIRECTIVES
        .iter()
        .find(|entry| entry.name == directive)
}

/// The directives that close a `@section`, and whether the section they
/// close renders itself where it stands.
///
/// `@show` is `@endsection` plus a `@yield` of the section just written,
/// so a layout's `@section('sidebar') … @show` declares the name as much
/// as a bare `@yield('sidebar')` does.
const SECTION_CLOSERS: &[(&str, bool)] = &[
    ("endsection", false),
    ("stop", false),
    ("append", false),
    ("overwrite", false),
    ("show", true),
];

/// The directives that render another template, and which argument names
/// it.
///
/// `@includeWhen` and `@includeUnless` take the condition first; every
/// other form leads with the view name.
const RENDER_DIRECTIVES: &[(&str, usize)] = &[
    ("include", 0),
    ("includeIf", 0),
    ("includeFirst", 0),
    ("includeIsolated", 0),
    ("includeWhen", 1),
    ("includeUnless", 1),
    ("each", 0),
    ("component", 0),
    ("componentFirst", 0),
];

/// What one template contributes to the render tree it sits in.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TemplateBlocks {
    /// Every section and stack name the template writes.
    pub(crate) blocks: Vec<BlockRef>,
    /// The layouts it extends, in the order Blade would try them.
    pub(crate) extends: Vec<String>,
    /// The templates it renders inline, by view name.
    pub(crate) includes: Vec<String>,
    /// Whether it renders something whose own names cannot be read: a view
    /// named by an expression, or a component tag. A caller that needs to
    /// know every name in the tree has to stand down when this is set,
    /// since the name it is looking for may be behind it.
    pub(crate) opaque: bool,
    /// Whether the template declares itself a component (`@props` /
    /// `@aware`). A component is rendered by a tag in another template, so
    /// the render tree it sits in continues above it in a direction
    /// `@extends` says nothing about.
    pub(crate) component: bool,
}

impl TemplateBlocks {
    /// Whether the template declares `name` for `kind`, or checks it: a
    /// layout that asks `@hasSection('sidebar')` consumes the section as
    /// surely as one that yields it.
    pub(crate) fn consumes(&self, kind: BlockKind, name: &str) -> bool {
        self.blocks.iter().any(|block| {
            block.kind == kind
                && block.name == name
                && matches!(block.role, BlockRole::Declare | BlockRole::Check)
        })
    }

    /// The names the template declares or checks for `kind`.
    pub(crate) fn consumed_names(&self, kind: BlockKind) -> impl Iterator<Item = &str> {
        self.blocks
            .iter()
            .filter(move |block| {
                block.kind == kind && matches!(block.role, BlockRole::Declare | BlockRole::Check)
            })
            .map(|block| block.name.as_str())
    }

    /// The names the template fills for `kind`.
    pub(crate) fn filled_names(&self, kind: BlockKind) -> impl Iterator<Item = &str> {
        self.blocks
            .iter()
            .filter(move |block| block.kind == kind && block.role == BlockRole::Fill)
            .map(|block| block.name.as_str())
    }
}

/// Read everything the pairing of section and stack names needs from one
/// template's source.
pub(crate) fn analyse(content: &str) -> TemplateBlocks {
    let mut out = TemplateBlocks::default();
    if !content.contains('@') {
        out.opaque = has_component_tag(content);
        out.component = super::signature::declares_component_directive(content);
        return out;
    }

    let masked = mask_regions(content, &inert_regions(content, true));
    // The `@section`s that are open, so the `@show` that closes one can go
    // back and mark it as rendering itself.
    let mut open_sections: Vec<Option<usize>> = Vec::new();

    for Directive {
        name: directive,
        span: directive_span,
        args,
    } in directives(&masked)
    {
        if let Some((_, renders)) = SECTION_CLOSERS
            .iter()
            .find(|(closer, _)| *closer == directive)
        {
            if let Some(Some(index)) = open_sections.pop()
                && *renders
                && let Some(block) = out.blocks.get_mut(index)
            {
                block.role = BlockRole::Declare;
            }
            continue;
        }

        let arguments = args
            .as_ref()
            .map(|args| inside(content, args))
            .unwrap_or_default();

        if let Some(entry) = named_block_directive(directive) {
            let split = split_top_level_args(arguments);
            // A `@section` with a second argument supplies its content
            // there and closes on the spot; only the one-argument form
            // opens a block a `@show` can close.
            let opens_block = directive == "section" && args.is_some() && split.len() < 2;
            let argument = match entry.argument {
                NameArgument::First => split.into_iter().next(),
                NameArgument::Last => split.into_iter().next_back(),
            };
            let read = argument.and_then(|argument| string_literal_span(content, argument));
            match read {
                Some((name, name_span)) => {
                    if opens_block {
                        open_sections.push(Some(out.blocks.len()));
                    }
                    out.blocks.push(BlockRef {
                        name,
                        kind: entry.kind,
                        role: entry.role,
                        name_span,
                        directive_span,
                    });
                }
                None => {
                    // A name built at runtime pairs with nothing that can
                    // be read, and hides which name was meant. The block it
                    // opens is still tracked, so the `@show` that closes it
                    // does not go looking for an outer section to render.
                    out.opaque = true;
                    if opens_block {
                        open_sections.push(None);
                    }
                }
            }
            continue;
        }

        if directive == "extends" || directive == "extendsFirst" {
            let names = view_names(content, directive, arguments);
            if names.is_empty() {
                out.opaque = true;
            }
            out.extends.extend(names);
            continue;
        }

        if let Some((_, index)) = RENDER_DIRECTIVES
            .iter()
            .find(|(render, _)| *render == directive)
        {
            let Some(argument) = split_top_level_args(arguments).into_iter().nth(*index) else {
                continue;
            };
            match string_literal_span(content, argument) {
                Some((name, _)) => out.includes.push(name),
                None => out.opaque = true,
            }
        }
    }

    out.opaque |= has_component_tag(content);
    out.component = super::signature::declares_component_directive(content);
    out
}

/// The view names an `@extends` / `@extendsFirst` argument list holds.
fn view_names(content: &str, directive: &str, arguments: &str) -> Vec<String> {
    let Some(first) = split_top_level_args(arguments).into_iter().next() else {
        return Vec::new();
    };
    if directive == "extendsFirst" {
        return super::signature::array_string_literals(first).unwrap_or_default();
    }
    string_literal_span(content, first)
        .map(|(name, _)| vec![name])
        .unwrap_or_default()
}

/// Whether the template renders a component tag, whose own template is not
/// something this scan can follow.
fn has_component_tag(content: &str) -> bool {
    content.contains("<x-") || content.contains("<livewire:")
}

/// A section or stack name the cursor is in the middle of writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockNameContext {
    pub(crate) kind: BlockKind,
    pub(crate) role: BlockRole,
    /// Where the name starts, just past its opening quote.
    pub(crate) name_start: usize,
}

/// The section or stack name the cursor at `offset` is inside, if it is
/// inside one.
///
/// This reads the raw template rather than the virtual PHP the rest of
/// completion works from, for the same reason directive-name completion
/// does: a name being typed is usually in a string literal that is not
/// closed yet, and the resulting edit has to land in the template's own
/// coordinates.
pub(crate) fn block_name_at(content: &str, offset: usize) -> Option<BlockNameContext> {
    let before = content.get(..offset)?;
    let bytes = before.as_bytes();

    // The opening quote of the literal being typed. Blade keeps a
    // directive's argument list on one line, so a newline means the cursor
    // is not in one.
    let mut quote = None;
    let mut i = offset;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'\n' => return None,
            b'\'' | b'"' => {
                quote = Some(i);
                break;
            }
            _ => {}
        }
    }
    let quote = quote?;
    // A name the template builds at runtime is not one to complete.
    if bytes[quote] == b'"' && before[quote + 1..].contains(['$', '{']) {
        return None;
    }

    // Back to the argument list's `(`, counting the arguments already
    // written. A literal on the way (a `@section`'s inline content, a
    // string inside the condition of a `@pushIf`) is where this gives up:
    // which argument the cursor is in stops being readable backwards.
    let mut depth = 0i32;
    let mut argument = 0usize;
    let mut j = quote;
    let open = loop {
        if j == 0 {
            return None;
        }
        j -= 1;
        match bytes[j] {
            b')' | b']' => depth += 1,
            b'(' if depth == 0 => break j,
            b'(' | b'[' => depth -= 1,
            b',' if depth == 0 => argument += 1,
            b'\'' | b'"' => return None,
            _ => {}
        }
    };

    // The directive name in front of it, and the `@` in front of that.
    let mut name_end = open;
    while matches!(bytes.get(name_end.wrapping_sub(1)), Some(b' ' | b'\t')) {
        name_end -= 1;
    }
    let mut name_start = name_end;
    while name_start > 0 && (bytes[name_start - 1].is_ascii_alphanumeric()) {
        name_start -= 1;
    }
    if name_start == 0 || bytes[name_start - 1] != b'@' {
        return None;
    }
    let at = name_start - 1;
    let entry = named_block_directive(&before[name_start..name_end])?;
    let expected = match entry.argument {
        NameArgument::First => argument == 0,
        // `@pushIf` leads with a condition, however many commas that
        // condition itself holds.
        NameArgument::Last => argument > 0,
    };
    if !expected {
        return None;
    }
    // A directive written in a comment, a `@verbatim`, or a `@php` block
    // is inert, and so is the name in it.
    if !super::directive_completion::is_html_position(content, at) {
        return None;
    }

    Some(BlockNameContext {
        kind: entry.kind,
        role: entry.role,
        name_start: quote + 1,
    })
}

/// The value and byte range of `argument`, when the whole of it is one
/// plain string literal.
///
/// `argument` is a slice of `content`, so the range it occupies is derived
/// from the two pointers rather than searched for: the same literal may
/// appear many times in a template.
///
/// A double-quoted literal that interpolates, and anything the literal is
/// only part of (`'layouts.' . $theme`), name nothing that can be read.
fn string_literal_span(content: &str, argument: &str) -> Option<(String, Span)> {
    let trimmed = argument.trim();
    let quote = trimmed
        .chars()
        .next()
        .filter(|ch| *ch == '\'' || *ch == '"')?;
    let value = &trimmed[1..trimmed[1..].find(quote)? + 1];
    if trimmed.len() != value.len() + 2 {
        return None;
    }
    if quote == '"' && value.contains(['$', '{']) {
        return None;
    }
    let start = value.as_ptr() as usize - content.as_ptr() as usize;
    Some((value.to_string(), start..start + value.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The blocks of `content` as short strings: `"declare section content"`.
    fn report(content: &str) -> Vec<String> {
        analyse(content)
            .blocks
            .into_iter()
            .map(|block| {
                let role = match block.role {
                    BlockRole::Declare => "declare",
                    BlockRole::Fill => "fill",
                    BlockRole::Check => "check",
                };
                format!("{role} {} {}", block.kind.label(), block.name)
            })
            .collect()
    }

    #[test]
    fn a_yield_declares_a_section_and_a_section_fills_one() {
        assert_eq!(report("@yield('content')"), ["declare section content"]);
        assert_eq!(
            report("@section('content')\nhi\n@endsection"),
            ["fill section content"]
        );
    }

    #[test]
    fn a_section_closed_by_show_renders_itself() {
        assert_eq!(
            report("@section('sidebar')\nhi\n@show"),
            ["declare section sidebar"]
        );
    }

    #[test]
    fn a_two_argument_section_is_a_fill_that_no_show_can_reach() {
        assert_eq!(
            report("@section('title', 'Home')\n@section('body')\nhi\n@show"),
            ["fill section title", "declare section body"]
        );
    }

    #[test]
    fn the_stack_directives_use_their_own_name_space() {
        assert_eq!(
            report("@stack('scripts')\n@push('scripts')x@endpush\n@hasStack('scripts')\n@endif"),
            [
                "declare stack scripts",
                "fill stack scripts",
                "check stack scripts",
            ]
        );
    }

    #[test]
    fn push_if_names_its_stack_last() {
        assert_eq!(
            report("@pushIf($ready, 'scripts')x@endPushIf"),
            ["fill stack scripts"]
        );
    }

    /// Laravel folds every comma before the last one into the condition, so
    /// a condition containing one still leaves the stack name last.
    #[test]
    fn push_if_reads_a_condition_that_contains_a_comma() {
        assert_eq!(
            report("@pushIf(in_array($a, $b), 'scripts')x@endPushIf"),
            ["fill stack scripts"]
        );
    }

    /// A `@show` renders the section its own `@section` opened. When that
    /// name is built at runtime there is nothing to record, and the block
    /// still has to be tracked or the `@show` would go looking for an outer
    /// section and mark that one as rendering itself.
    #[test]
    fn a_show_closing_a_dynamic_section_leaves_the_outer_one_alone() {
        assert_eq!(
            report("@section('outer')\n@section($name)\nx\n@show\n@endsection"),
            ["fill section outer"]
        );
    }

    #[test]
    fn the_section_helpers_check_rather_than_fill() {
        assert_eq!(
            report("@hasSection('nav')\n@endif\n@sectionMissing('nav')\n@endif"),
            ["check section nav", "check section nav"]
        );
    }

    #[test]
    fn a_name_inside_an_inert_region_names_nothing() {
        assert_eq!(report("{{-- @yield('content') --}}"), Vec::<String>::new());
        assert_eq!(
            report("@verbatim @yield('content') @endverbatim"),
            Vec::<String>::new()
        );
        assert_eq!(
            report("@php $x = \"@yield('content')\"; @endphp"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_name_span_covers_the_name_without_its_quotes() {
        let content = "<div>@yield('content')</div>";
        let blocks = analyse(content).blocks;
        assert_eq!(&content[blocks[0].name_span.clone()], "content");
    }

    #[test]
    fn the_extends_target_and_the_include_targets_are_recorded() {
        let blocks = analyse(
            "@extends('layouts.app')\n@include('partials.head')\n@includeWhen($a, 'partials.nav')\n",
        );
        assert_eq!(blocks.extends, ["layouts.app"]);
        assert_eq!(blocks.includes, ["partials.head", "partials.nav"]);
        assert!(!blocks.opaque);
    }

    #[test]
    fn extends_first_records_every_candidate() {
        assert_eq!(
            analyse("@extendsFirst(['themes.dark', 'layouts.app'])").extends,
            ["themes.dark", "layouts.app"]
        );
    }

    #[test]
    fn a_render_target_that_cannot_be_read_makes_the_template_opaque() {
        assert!(analyse("@include($partial)").opaque);
        assert!(analyse("@extends($layout)").opaque);
        assert!(analyse("@yield($name)").opaque);
        assert!(analyse("<x-alert>hi</x-alert>").opaque);
        assert!(!analyse("@include('partials.head')").opaque);
    }

    /// Every entry of the table has to name a directive the preprocessor
    /// recognises, or the marker it lowers to would never be emitted.
    #[test]
    fn every_named_block_directive_is_a_known_directive() {
        for entry in NAMED_BLOCK_DIRECTIVES {
            assert_eq!(
                super::super::directives::match_directive(entry.name),
                Some(entry.name),
                "{:?} is not a known directive",
                entry.name
            );
        }
    }
}
