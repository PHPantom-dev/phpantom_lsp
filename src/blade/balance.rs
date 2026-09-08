//! Whether a template's block directives pair up.
//!
//! Blade compiles every directive independently by name: `@foreach` becomes
//! a `foreach (…):` and `@endif` an `endif;`, with nothing checking that the
//! two belong together. A template that closes a loop with the wrong
//! directive therefore compiles without complaint and fails at render time,
//! with the error pointing at the compiled cache file rather than at the
//! template. The same goes for a block nobody closes.
//!
//! This scan reads the raw Blade source (not the virtual PHP the
//! preprocessor emits) and walks the directive stream with a stack of open
//! blocks. The regions Blade itself excludes from directive processing —
//! comments, `@verbatim`, and `@php` blocks — are masked out first by
//! [`super::signature::inert_regions`], the same scan the `@props` and
//! component-tag readers use, so an `@endif` written in prose or in PHP
//! code opens and closes nothing here either.

use std::ops::Range;

use super::directives::match_directive;
use super::pairing::{self, Pair, Stray, Token};
use super::signature::{
    InertOpener, inert_regions, mask_regions, matching_paren, split_top_level_args,
};

/// A byte range of the original Blade source.
pub(crate) type Span = Range<usize>;

/// A place where the block structure does not add up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Imbalance {
    /// A closing directive that does not close the innermost open block.
    Mismatched {
        closer: Span,
        found: &'static str,
        expected: &'static str,
        opener: &'static str,
        opener_span: Span,
    },
    /// A closing directive with no open block to close.
    Unexpected {
        closer: Span,
        found: &'static str,
        opener: &'static str,
    },
    /// A block the template never closes.
    Unclosed {
        opener_span: Span,
        opener: &'static str,
        expected: &'static str,
    },
}

impl Imbalance {
    /// The range the report is anchored on: the offending directive
    /// itself.
    pub(crate) fn span(&self) -> &Span {
        match self {
            Imbalance::Mismatched { closer, .. } | Imbalance::Unexpected { closer, .. } => closer,
            Imbalance::Unclosed { opener_span, .. } => opener_span,
        }
    }
}

/// When a directive opens a block that has to be closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opens {
    /// Always, argument list or not: `@once`, `@auth`, `@auth('admin')`.
    Always,
    /// Only with an argument list. Blade has nothing to compile without
    /// one, and the bare name in a template is something else: `@empty` is
    /// `@forelse`'s separator, and an `@error="…"` in markup is a
    /// JavaScript framework's event binding.
    WithArgs,
    /// `@section('sidebar')` opens a section; the two-argument
    /// `@section('title', 'Home')` is a complete statement on its own.
    Section,
    /// `@lang` and `@lang(['count' => 1])` open a translation block, while
    /// `@lang('messages.welcome')` echoes one string.
    Lang,
    /// Never, because the region scan consumes the whole block before the
    /// stack is built. The entry exists only to name the opener a stray
    /// closer is missing.
    Never,
}

/// One block-structuring directive: what opens it, what Blade accepts as
/// its close, and when the opener actually opens a block.
///
/// Laravel compiles most of these closers to a bare `endif;`, so its own
/// compiler would accept any of them anywhere. The pairing below is the one
/// Laravel's documentation gives and the one every Blade-aware editor
/// checks, which is what an author means when they write it.
struct Block {
    opener: &'static str,
    closers: &'static [&'static str],
    opens: Opens,
}

const BLOCKS: &[Block] = &[
    Block {
        opener: "if",
        closers: &["endif"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "unless",
        closers: &["endunless"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "isset",
        closers: &["endisset"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "empty",
        closers: &["endempty"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "foreach",
        closers: &["endforeach"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "forelse",
        closers: &["endforelse"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "for",
        closers: &["endfor"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "while",
        closers: &["endwhile"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "switch",
        closers: &["endswitch"],
        opens: Opens::WithArgs,
    },
    // Laravel compiles all three to a plain `if`, so `@endif` is what
    // closes them.
    Block {
        opener: "hasSection",
        closers: &["endif"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "sectionMissing",
        closers: &["endif"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "hasStack",
        closers: &["endif"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "auth",
        closers: &["endauth"],
        opens: Opens::Always,
    },
    Block {
        opener: "guest",
        closers: &["endguest"],
        opens: Opens::Always,
    },
    Block {
        opener: "production",
        closers: &["endproduction"],
        opens: Opens::Always,
    },
    Block {
        opener: "env",
        closers: &["endenv"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "session",
        closers: &["endsession"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "context",
        closers: &["endcontext"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "error",
        closers: &["enderror"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "can",
        closers: &["endcan"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "cannot",
        closers: &["endcannot"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "canany",
        closers: &["endcanany"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "once",
        closers: &["endonce"],
        opens: Opens::Always,
    },
    Block {
        opener: "fragment",
        closers: &["endfragment"],
        opens: Opens::WithArgs,
    },
    // A section ends with any of the four directives that publish it.
    Block {
        opener: "section",
        closers: &["endsection", "stop", "show", "append", "overwrite"],
        opens: Opens::Section,
    },
    Block {
        opener: "push",
        closers: &["endpush"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "pushIf",
        closers: &["endPushIf"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "pushOnce",
        closers: &["endPushOnce"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "prepend",
        closers: &["endprepend"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "prependOnce",
        closers: &["endPrependOnce"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "component",
        closers: &["endcomponent"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "componentFirst",
        closers: &["endcomponentFirst"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "slot",
        closers: &["endslot"],
        opens: Opens::WithArgs,
    },
    Block {
        opener: "lang",
        closers: &["endlang"],
        opens: Opens::Lang,
    },
    Block {
        opener: "verbatim",
        closers: &["endverbatim"],
        opens: Opens::Never,
    },
    Block {
        opener: "php",
        closers: &["endphp"],
        opens: Opens::Never,
    },
];

/// What one walk of a template's directive stream finds.
#[derive(Debug, Default)]
pub(crate) struct Balance {
    /// Every well-nested block pair, opener through closer.
    pub(crate) pairs: Vec<BlockPair>,
    /// Every place the block structure does not add up, in source order.
    pub(crate) imbalances: Vec<Imbalance>,
}

/// A directive that opened a block, waiting for its closer.
#[derive(Clone)]
struct Opened {
    block: &'static Block,
    span: Span,
    args: Option<Span>,
}

/// A closing directive: the name it was written as, and the block whose
/// closer it is (the first, when several blocks share it), which names
/// the opener a stray closer is missing.
struct Closer {
    name: &'static str,
    block: &'static Block,
    span: Span,
}

/// Read the directive stream once and pair it up with
/// [`pairing::pair`], collecting both the blocks that pair and the ones
/// that do not.
///
/// Pairing and reporting have to agree on what a closer closes, so both
/// come out of this one walk: a closer that does not match the innermost
/// open block ends that block anyway (with a report, and without a pair),
/// so that no later closer can pair with an opener a stray closer already
/// consumed.
pub(crate) fn walk(content: &str) -> Balance {
    let mut balance = Balance::default();
    if !content.contains('@') {
        return balance;
    }

    let regions = inert_regions(content, true);
    let masked = mask_regions(content, &regions);

    let mut tokens: Vec<Token<Opened, Closer>> = Vec::new();
    for Directive { name, span, args } in directives(&masked) {
        if let Some(block) = BLOCKS.iter().find(|block| block.opener == name) {
            if opens_block(block, &masked, args.as_ref()) {
                tokens.push(Token::Open(Opened { block, span, args }));
            }
            continue;
        }
        if let Some(block) = BLOCKS.iter().find(|block| block.closers.contains(&name)) {
            tokens.push(Token::Close(Closer { name, block, span }));
        }
    }

    let pairing = pairing::pair(tokens, |open: &Opened, closer: &Closer| {
        open.block.closers.contains(&closer.name)
    });

    balance.pairs = pairing
        .pairs
        .into_iter()
        .map(|Pair { opener, closer }| BlockPair {
            opener: opener.span,
            args: opener.args,
            closer: closer.span,
        })
        .collect();
    for stray in pairing.strays {
        balance.imbalances.push(match stray {
            Stray::Mismatched { closer, skipped } => Imbalance::Mismatched {
                closer: closer.span,
                found: closer.name,
                expected: skipped.block.closers[0],
                opener: skipped.block.opener,
                opener_span: skipped.span,
            },
            Stray::Unexpected { closer } => Imbalance::Unexpected {
                closer: closer.span,
                found: closer.name,
                opener: closer.block.opener,
            },
        });
    }

    // An unterminated `@verbatim` or `@php` swallows the rest of the
    // template, so everything still open at this point is open only
    // because its closer was eaten. Report the region that ate them and
    // leave the open blocks alone.
    let unterminated = regions
        .iter()
        .find(|region| !region.terminated && region.opener != InertOpener::Comment);
    if let Some(region) = unterminated {
        let (opener, expected) = match region.opener {
            InertOpener::Verbatim => ("verbatim", "endverbatim"),
            _ => ("php", "endphp"),
        };
        balance.imbalances.push(Imbalance::Unclosed {
            opener_span: region.span.start..region.span.start + 1 + opener.len(),
            opener,
            expected,
        });
    } else {
        for opened in pairing.open {
            balance.imbalances.push(Imbalance::Unclosed {
                opener_span: opened.span,
                opener: opened.block.opener,
                expected: opened.block.closers[0],
            });
        }
    }

    balance
        .imbalances
        .sort_by_key(|imbalance| imbalance.span().start);
    balance
}

/// Every block directive in `content` that does not pair up.
pub(crate) fn check(content: &str) -> Vec<Imbalance> {
    walk(content).imbalances
}

/// A block directive and the closer that ends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockPair {
    /// The opener itself (`@section`), without its argument list.
    pub(crate) opener: Span,
    /// The opener's argument list, parentheses included, when it has one.
    pub(crate) args: Option<Span>,
    /// The directive that closes the block (`@endsection`).
    pub(crate) closer: Span,
}

/// Every well-nested block-directive pair in `content`, covering
/// `@directive(...)` through its matching `@enddirective`.
///
/// Read from the same walk as [`check`], so the two agree on what a
/// closer closes: a closer that does not match the innermost open
/// block (already reported by [`check`]) pairs with nothing.
pub(crate) fn block_pairs(content: &str) -> Vec<BlockPair> {
    walk(content).pairs
}

/// Whether an occurrence of `block`'s opener with `args` opens a block.
fn opens_block(block: &Block, content: &str, args: Option<&Span>) -> bool {
    match block.opens {
        Opens::Always => true,
        Opens::WithArgs => args.is_some(),
        Opens::Section => {
            args.is_some_and(|args| split_top_level_args(inside(content, args)).len() < 2)
        }
        Opens::Lang => args.is_none_or(|args| inside(content, args).trim_start().starts_with('[')),
        Opens::Never => false,
    }
}

/// The argument text between a directive's parentheses.
pub(crate) fn inside<'a>(content: &'a str, args: &Span) -> &'a str {
    content
        .get(args.start + 1..args.end - 1)
        .unwrap_or_default()
}

/// One directive of a template, as [`directives`] yields it.
pub(crate) struct Directive {
    pub(crate) name: &'static str,
    /// The `@` and the name, without the argument list.
    pub(crate) span: Span,
    /// The argument list, parentheses included, when it has one.
    pub(crate) args: Option<Span>,
}

/// Every directive in `masked`, in document order.
///
/// `masked` is the template with its inert regions blanked out by
/// [`mask_regions`], so nothing written in a comment, a `@verbatim`, or a
/// `@php` block is yielded. The scan resumes past each directive's
/// argument list, so a name written inside one
/// (`@include('partials.@endif')`) is not read as a directive of its own.
pub(crate) fn directives(masked: &str) -> impl Iterator<Item = Directive> + '_ {
    let bytes = masked.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while let Some(at) = bytes[i..].iter().position(|byte| *byte == b'@') {
            i += at;
            let Some((name, args)) = directive_at(masked, i) else {
                i += 1;
                continue;
            };
            let span = i..i + 1 + name.len();
            i = args.as_ref().map_or(span.end, |args| args.end);
            return Some(Directive { name, span, args });
        }
        None
    })
}

/// The directive at `at` (which is on an `@`), and the byte range of its
/// argument list, parentheses included, when it has one.
///
/// Blade's own `compileStatements` pattern is anchored with `\B`, so a name
/// glued to a preceding word is not a directive: an `@production` in
/// `admin@production.example` compiles to nothing, and `@@if` is the escape
/// for a literal `@if`.
fn directive_at(content: &str, at: usize) -> Option<(&'static str, Option<Span>)> {
    let bytes = content.as_bytes();
    if at > 0 && (bytes[at - 1] == b'@' || is_word_byte(bytes[at - 1])) {
        return None;
    }
    let name = match_directive(content.get(at + 1..)?)?;
    let after = at + 1 + name.len();
    // `@error="…"` and `@class="…"` are a JavaScript framework's bindings
    // written in markup. Blade has no directive form that runs a name into
    // an `=`, so neither opens nor closes anything.
    if bytes.get(after) == Some(&b'=') {
        return None;
    }
    // Blade allows spaces and tabs, but no newline, between a directive
    // name and its opening parenthesis.
    let mut open = after;
    while matches!(bytes.get(open), Some(b' ' | b'\t')) {
        open += 1;
    }
    if bytes.get(open) != Some(&b'(') {
        return Some((name, None));
    }
    // An unterminated argument list is a template mid-edit; the directive
    // is read without one rather than swallowing the rest of the file.
    Some((name, matching_paren(bytes, open).map(|end| open..end + 1)))
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The imbalances of `content` as short strings, for readable
    /// assertions: `"mismatched endif/endforeach"`, `"unexpected endif"`,
    /// `"unclosed if"`.
    fn report(content: &str) -> Vec<String> {
        check(content)
            .into_iter()
            .map(|imbalance| match imbalance {
                Imbalance::Mismatched {
                    found, expected, ..
                } => format!("mismatched {found}/{expected}"),
                Imbalance::Unexpected { found, .. } => format!("unexpected {found}"),
                Imbalance::Unclosed { opener, .. } => format!("unclosed {opener}"),
            })
            .collect()
    }

    #[test]
    fn paired_directives_report_nothing() {
        let blade = "@foreach ($rows as $row)\n\
                     @if ($row->visible)\n\
                     <p>{{ $row->name }}</p>\n\
                     @else\n\
                     <p>hidden</p>\n\
                     @endif\n\
                     @endforeach\n\
                     @forelse ($rows as $row)\n{{ $row }}\n@empty\nnone\n@endforelse\n\
                     @section('body')\n@show\n\
                     @push('scripts')\n@endpush\n\
                     @once\n@endonce\n\
                     @auth\n@endauth\n\
                     @can('edit', $post)\n@endcan\n";
        assert!(report(blade).is_empty(), "{:?}", report(blade));
    }

    #[test]
    fn a_closer_for_another_block_is_mismatched() {
        assert_eq!(
            report("@foreach ($rows as $row)\n@endif\n"),
            ["mismatched endif/endforeach"]
        );
    }

    #[test]
    fn a_closer_with_nothing_open_is_unexpected() {
        assert_eq!(report("<p>hi</p>\n@endif\n"), ["unexpected endif"]);
    }

    #[test]
    fn a_block_the_template_never_closes_is_reported() {
        assert_eq!(report("@if ($ok)\n<p>hi</p>\n"), ["unclosed if"]);
    }

    /// The `@empty` of a `@forelse` is a separator, but `@empty($rows)` is
    /// a block of its own.
    #[test]
    fn empty_is_read_by_its_argument_list() {
        assert!(report("@forelse ($rows as $row)\n@empty\n@endforelse\n").is_empty());
        assert_eq!(report("@empty($rows)\nnone\n"), ["unclosed empty"]);
        assert!(report("@empty($rows)\nnone\n@endempty\n").is_empty());
    }

    /// The two-argument `@section` is a complete statement; the
    /// one-argument form opens a block.
    #[test]
    fn a_two_argument_section_opens_nothing() {
        assert!(report("@section('title', 'Home')\n").is_empty());
        assert_eq!(report("@section('body')\n"), ["unclosed section"]);
        assert!(report("@section('body')\n@endsection\n").is_empty());
    }

    /// `@lang('key')` echoes a string; the bare and array forms buffer a
    /// translation block.
    #[test]
    fn lang_is_read_by_its_argument_list() {
        assert!(report("@lang('messages.welcome')\n").is_empty());
        assert_eq!(report("@lang\nhello\n"), ["unclosed lang"]);
        assert!(report("@lang\nhello\n@endlang\n").is_empty());
    }

    /// A directive is only a directive where Blade compiles one: not in a
    /// comment, a `@verbatim` block, a `@php` block, an escaped `@@if`,
    /// glued to a word, or as a markup attribute name.
    #[test]
    fn text_that_only_looks_like_a_directive_is_left_alone() {
        assert!(report("{{-- @if ($ok) --}}\n").is_empty());
        assert!(report("@verbatim\n@if\n@endif\n@endverbatim\n").is_empty());
        assert!(report("@php\n// @endif\n@endphp\n").is_empty());
        assert!(report("@@if ($ok)\n").is_empty());
        assert!(report("<a href=\"mailto:admin@production.example\">x</a>\n").is_empty());
        assert!(report("<img @error=\"fallback()\">\n").is_empty());
        assert!(report("@include('partials.@endif')\n").is_empty());
    }

    /// An unterminated `@verbatim` eats every directive after it, so it is
    /// the only thing worth reporting.
    #[test]
    fn an_unterminated_inert_region_is_the_only_report() {
        assert_eq!(
            report("@if ($ok)\n@verbatim\n@endif\n"),
            ["unclosed verbatim"]
        );
        assert_eq!(report("@php\n$x = 1;\n"), ["unclosed php"]);
    }

    /// A stray closer for a region the scan consumed whole still has an
    /// opener to name.
    #[test]
    fn a_stray_region_closer_is_unexpected() {
        assert_eq!(
            report("<p>hi</p>\n@endverbatim\n"),
            ["unexpected endverbatim"]
        );
        assert_eq!(report("@php($x = 1)\n@endphp\n"), ["unexpected endphp"]);
    }

    /// A closer that closes a block further out is that block's close, and
    /// what it skipped past is the one report: the `@if` here is what has
    /// no end, not the `@section` the `@show` publishes.
    #[test]
    fn a_closer_matching_a_deeper_block_reports_what_it_skipped() {
        assert_eq!(
            report("@section('body')\n@if ($ok)\n<p>hi</p>\n@show\n"),
            ["mismatched show/endif"]
        );
    }

    /// Every closer in the table has to be a directive the preprocessor
    /// recognises, or a template using it would never reach this scan.
    #[test]
    fn every_block_name_is_a_known_directive() {
        for block in BLOCKS {
            assert_eq!(
                match_directive(block.opener),
                Some(block.opener),
                "opener {:?} is not a known directive",
                block.opener
            );
            for closer in block.closers {
                assert_eq!(
                    match_directive(closer),
                    Some(*closer),
                    "closer {closer:?} is not a known directive"
                );
            }
        }
    }

    /// The stream resumes past each argument list, so a directive name
    /// written inside one belongs to that argument list rather than
    /// being a directive of its own.
    #[test]
    fn a_directive_name_inside_an_argument_list_is_not_yielded() {
        let blade = "@include('partials.@endif')\n@if ($ok)\n@endif\n";
        let found: Vec<_> = directives(blade)
            .map(|directive| {
                (
                    directive.name,
                    &blade[directive.span],
                    directive.args.map(|args| &blade[args]),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("include", "@include", Some("('partials.@endif')")),
                ("if", "@if", Some("($ok)")),
                ("endif", "@endif", None),
            ]
        );
    }

    /// `block_pairs` spans as `(name, opener_start, closer_end)` triples,
    /// for readable assertions.
    fn pairs(content: &str) -> Vec<(&str, usize, usize)> {
        block_pairs(content)
            .into_iter()
            .map(|pair| {
                (
                    &content[pair.opener.start + 1..pair.opener.end],
                    pair.opener.start,
                    pair.closer.end,
                )
            })
            .collect()
    }

    #[test]
    fn a_paired_block_spans_from_its_opener_to_its_closer() {
        let blade = "@foreach ($rows as $row)\n<p>{{ $row }}</p>\n@endforeach\n";
        assert_eq!(pairs(blade), [("foreach", 0, blade.len() - 1)]);
    }

    /// The argument list travels with the pair, so a reader that has to
    /// know *which* section a block opens does not rescan for it.
    #[test]
    fn a_pair_carries_the_openers_argument_list() {
        let blade = "@section('body')\n<p>hi</p>\n@endsection\n";
        let args = block_pairs(blade)[0]
            .args
            .clone()
            .expect("section has args");
        assert_eq!(&blade[args], "('body')");
    }

    #[test]
    fn nested_blocks_each_pair_independently() {
        let blade = "@if ($ok)\n@foreach ($rows as $row)\n{{ $row }}\n@endforeach\n@endif\n";
        let found = pairs(blade);
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|(name, ..)| *name == "if"));
        assert!(found.iter().any(|(name, ..)| *name == "foreach"));
    }

    #[test]
    fn a_mismatched_closer_pairs_neither_side() {
        assert!(pairs("@foreach ($rows as $row)\n@endif\n").is_empty());
    }

    /// A stray closer ends the block it sits in for pairing as well as for
    /// reporting, so the block's own closer, coming later, pairs with
    /// nothing rather than reaching back past the stray one.
    #[test]
    fn a_stray_closer_consumes_the_block_for_pairing_too() {
        let blade = "@foreach ($rows as $row)\n@endif\n@endforeach\n";
        assert!(pairs(blade).is_empty());
        assert_eq!(
            report(blade),
            ["mismatched endif/endforeach", "unexpected endforeach"]
        );
    }

    #[test]
    fn an_unclosed_block_does_not_pair() {
        assert!(pairs("@if ($ok)\n<p>hi</p>\n").is_empty());
    }
}
