//! Keeping the braced `namespace` blocks mago's parser drops whole.
//!
//! The parser recovers from a broken statement in an unbraced
//! `namespace App;` body by dropping just that statement.  A braced
//! `namespace App { … }` block that runs to the end of the file, because an
//! unclosed call swallowed its `}` or the `}` has not been typed yet, is
//! dropped whole instead, and every import, class and function it declares
//! goes with it.  A name written in it then resolves against whichever
//! block the parse kept last.
//!
//! [`reparse_dropped_namespace_blocks`] parses such a file again with the
//! dropped block's `{` replaced by `;`.  That opens the block unbraced, so
//! the parser recovers it statement by statement, and leaves every offset
//! where it was, so the reparse describes the text the caller holds.

use mago_allocator::{Arena, LocalArena};
use mago_span::HasSpan;
use mago_syntax::cst::{Program, Trivia};

/// Parse `content` again with each braced `namespace` block its parse,
/// `program`, dropped whole opened unbraced, or `None` when it dropped
/// none.
///
/// The reparse and the text it reads are allocated in `arena`.  Its
/// `errors` describe that text, which lacks the braces the blocks were
/// opened with, so a caller reporting syntax errors takes them from
/// `program`.
pub(crate) fn reparse_dropped_namespace_blocks<'a>(
    arena: &'a LocalArena,
    program: &'a Program<'a>,
    content: &str,
) -> Option<&'a Program<'a>> {
    // A file that parsed without complaint dropped nothing.
    if program.errors.is_empty() {
        return None;
    }
    let mut brace = first_dropped_namespace_brace(program, content)?;
    let mut text = content.to_string();
    let mut kept = None;
    loop {
        text.replace_range(brace..=brace, ";");
        let source: &str = arena.alloc_str(&text);
        // The text is still malformed, and the parser panics on some
        // malformed text.  A parse that got through stands then.
        let Some(reparsed) = super::recovery::parse_keeping_end(arena, program.file_id, source)
        else {
            return kept;
        };
        kept = Some(reparsed);
        // A dropped block can swallow the blocks after it, which the
        // reparse reads afresh and may drop in turn.  Each round opens a
        // block further down, so this ends once none is dropped.
        match first_dropped_namespace_brace(reparsed, source) {
            Some(next) if next > brace => brace = next,
            _ => return kept,
        }
    }
}

/// The offset of the `{` opening the first braced `namespace` block the
/// parse of `content` dropped.
///
/// The parser keeps or drops each top-level statement whole, so code
/// between two statements it kept, or after the last, is one it dropped.
fn first_dropped_namespace_brace(program: &Program<'_>, content: &str) -> Option<usize> {
    let trivia = program.trivia.as_slice();
    let bytes = content.as_bytes();
    let mut gap_start = 0;
    for statement in program.statements.iter() {
        let span = statement.span();
        let gap = &bytes[..span.start.offset as usize];
        if let Some(brace) = namespace_brace_at(trivia, gap, gap_start) {
            return Some(brace);
        }
        gap_start = gap_start.max(code_after(bytes, span.end.offset as usize));
    }
    namespace_brace_at(trivia, bytes, gap_start)
}

/// Where the code after a statement ending at `end` starts.
///
/// A statement the parser ends without its `;` or `)` ends on the token it
/// stopped at, which it leaves for the next statement.  When that token is
/// the `namespace` of the next block, the next statement starts on it.
fn code_after(bytes: &[u8], end: usize) -> usize {
    let keyword = end.saturating_sub(KEYWORD.len());
    let ends_on_keyword = bytes[keyword..end].eq_ignore_ascii_case(KEYWORD)
        && !keyword
            .checked_sub(1)
            .is_some_and(|before| is_name_byte(bytes[before]) || bytes[before] == b'$');
    if ends_on_keyword { keyword } else { end }
}

const KEYWORD: &[u8] = b"namespace";

/// The offset of the `{` after a `namespace` header, when the code from
/// `from` to the end of `bytes` starts with one.
fn namespace_brace_at(trivia: &[Trivia<'_>], bytes: &[u8], from: usize) -> Option<usize> {
    let keyword = next_code(trivia, bytes, from)?;
    let after = keyword + KEYWORD.len();
    if !bytes.get(keyword..after)?.eq_ignore_ascii_case(KEYWORD)
        // A longer name, or the `namespace\foo()` operator.
        || bytes.get(after).is_some_and(|&b| is_name_byte(b))
    {
        return None;
    }
    let mut at = next_code(trivia, bytes, after)?;
    let name_len = bytes[at..].iter().take_while(|&&b| is_name_byte(b)).count();
    if name_len > 0 {
        at = next_code(trivia, bytes, at + name_len)?;
    }
    (bytes[at] == b'{').then_some(at)
}

/// The first offset from `at` on that is neither whitespace nor inside a
/// comment.
fn next_code(trivia: &[Trivia<'_>], bytes: &[u8], mut at: usize) -> Option<usize> {
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let next = trivia.partition_point(|t| t.span.end.offset as usize <= at);
        match trivia.get(next) {
            Some(t) if t.span.start.offset as usize <= at => at = t.span.end.offset as usize,
            _ => return Some(at),
        }
    }
    None
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b >= 0x80
}

#[cfg(test)]
mod tests {
    use mago_syntax::cst::Statement;
    use mago_syntax::parser::parse_file_content;

    use super::*;

    use crate::parser::recovery::parse_program;

    /// The name of each top-level `namespace` block the parse keeps, with
    /// the text of the statements in it.
    fn kept_blocks(content: &str) -> Vec<(String, Vec<&str>)> {
        let arena = LocalArena::new();
        parse_program(&arena, content)
            .statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Namespace(ns) => Some((
                    ns.name.as_ref().map_or(String::new(), |name| {
                        crate::atom::bytes_to_str(name.value()).to_string()
                    }),
                    ns.statements()
                        .iter()
                        .map(|inner| {
                            let span = inner.span();
                            &content[span.start.offset as usize..span.end.offset as usize]
                        })
                        .collect(),
                )),
                _ => None,
            })
            .collect()
    }

    fn block_names(content: &str) -> Vec<String> {
        kept_blocks(content)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn reparses(content: &str) -> bool {
        let arena = LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = parse_file_content(&arena, file_id, content.as_bytes());
        reparse_dropped_namespace_blocks(&arena, program, content).is_some()
    }

    #[test]
    fn an_unclosed_call_keeps_the_block_it_is_in() {
        let blocks = kept_blocks(concat!(
            "<?php\n",
            "namespace Lib {\n",
            "    function greet(string $name): void {}\n",
            "}\n",
            "namespace App {\n",
            "    use function Lib\\greet;\n",
            "    class Widget {}\n",
            "    greet(\n",
            "}\n",
        ));
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_eq!(blocks[1].0, "App");
        // Read at the reparse's offsets in the text as written.
        assert!(
            blocks[1].1.contains(&"use function Lib\\greet;"),
            "{blocks:?}"
        );
        assert!(blocks[1].1.contains(&"class Widget {}"), "{blocks:?}");
    }

    #[test]
    fn a_block_missing_its_closing_brace_is_kept() {
        let blocks = kept_blocks("<?php\nnamespace App {\n    class Widget {}\n");
        assert_eq!(blocks, [("App".to_string(), vec!["class Widget {}"])]);
    }

    #[test]
    fn the_blocks_a_dropped_block_swallowed_are_kept() {
        let content = concat!(
            "<?php\n",
            "namespace App {\n",
            "    greet($a, \n",
            "}\n",
            "namespace Lib {\n",
            "    class Widget {}\n",
            "}\n",
        );
        let blocks = kept_blocks(content);
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_eq!(blocks[1], ("Lib".to_string(), vec!["class Widget {}"]));
    }

    #[test]
    fn every_dropped_block_is_kept() {
        let content = concat!(
            "<?php\n",
            "namespace A {\n",
            "    greet(\n",
            "}\n",
            "namespace B {\n",
            "    class Widget {}\n",
            "    greet(\n",
            "}\n",
        );
        assert_eq!(block_names(content), ["A", "B"]);
    }

    #[test]
    fn comments_around_the_header_are_read_past() {
        let content = concat!(
            "<?php\n",
            "/** The app. */\n",
            "namespace /* name */ App // the brace is next\n",
            "{\n",
            "    greet(\n",
            "}\n",
        );
        assert_eq!(block_names(content), ["App"]);
    }

    #[test]
    fn the_global_namespace_block_is_kept() {
        assert_eq!(block_names("<?php\nnamespace {\n    greet(\n}\n"), [""]);
    }

    #[test]
    fn only_a_dropped_braced_block_is_reparsed() {
        assert!(!reparses("<?php\nnamespace App {\n    greet();\n}\n"));
        // An unbraced block drops only the broken statement.
        assert!(!reparses("<?php\nnamespace App;\ngreet(\n"));
        assert!(!reparses(
            "<?php\nclass Widget {\n    function f() { greet( }\n}\n"
        ));
        // The `namespace\greet()` operator opens no block.
        assert!(!reparses("<?php\nnamespace\\greet(\n"));
    }
}
