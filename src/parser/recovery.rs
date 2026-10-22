//! Parsing again the code mago's parser drops while the file is being
//! edited.
//!
//! Every reparse keeps what the end of the file cuts off: a statement
//! still missing its `)` or `;`, or a body still missing its `}`, is kept
//! up to the end of the file, with the missing closers given empty spans
//! there.  The other steps rewrite a few bytes of the text without moving
//! any offset, so the reparse describes the text the caller holds:
//! [`bracket_recovery`](super::bracket_recovery) closes the brackets an
//! unfinished expression leaves open before a `}`, and
//! [`namespace_recovery`](super::namespace_recovery) opens unbraced the
//! braced `namespace` blocks that a `}` taken for an expression leaves
//! running to the end of the file.

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_syntax::cst::Program;
use mago_syntax::error::ParseError;
use mago_syntax::parser::{parse_file_content, parse_file_content_with_settings};
use mago_syntax::settings::ParserSettings;

/// Parse `content`, keeping the code the parser would drop (see
/// [`reparse_dropped_code`]).
pub(crate) fn parse_program<'a>(arena: &'a LocalArena, content: &'a str) -> &'a Program<'a> {
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(arena, file_id, content.as_bytes());
    reparse_dropped_code(arena, program, content).unwrap_or(program)
}

/// Parse `content` again keeping the code its parse, `program`, dropped,
/// or `None` when it dropped none it could keep.
///
/// The reparse and the text it reads are allocated in `arena`.  Its
/// `errors` describe that text, so a caller reporting syntax errors takes
/// them from `program`.
pub(crate) fn reparse_dropped_code<'a>(
    arena: &'a LocalArena,
    program: &'a Program<'a>,
    content: &str,
) -> Option<&'a Program<'a>> {
    // A file that parsed without complaint dropped nothing.
    if program.errors.is_empty() {
        return None;
    }
    let cut_off = program
        .errors
        .iter()
        .any(|error| matches!(error, ParseError::UnexpectedEndOfFile(..)));
    let kept_to_end = if cut_off {
        parse_keeping_end(arena, program.file_id, content)
    } else {
        None
    };
    let program = kept_to_end.unwrap_or(program);
    let closed = super::bracket_recovery::close_open_brackets(arena, program, content);
    let (parse, text) = closed.map_or((program, content), |(parse, text)| (parse, text));
    super::namespace_recovery::reparse_dropped_namespace_blocks(arena, parse, text)
        .or(closed.map(|(parse, _)| parse))
        .or(kept_to_end)
}

/// Parse `content` keeping what the end of the file cuts off, or `None`
/// when the parser panics, as it does on some malformed text.
pub(super) fn parse_keeping_end<'a>(
    arena: &'a LocalArena,
    file_id: FileId,
    content: &str,
) -> Option<&'a Program<'a>> {
    let settings = ParserSettings {
        recover_incomplete: true,
        ..ParserSettings::default()
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parse_file_content_with_settings(arena, file_id, content.as_bytes(), settings)
    }))
    .ok()
}

#[cfg(test)]
mod tests {
    use mago_span::HasSpan;
    use mago_syntax::cst::Statement;

    use super::*;

    /// The text of each statement the parse keeps, inside `namespace`
    /// blocks too.
    fn kept_statements(content: &str) -> Vec<&str> {
        let arena = LocalArena::new();
        let mut kept = Vec::new();
        for statement in parse_program(&arena, content).statements.iter() {
            let inner: Vec<&Statement<'_>> = match statement {
                Statement::Namespace(ns) => ns.statements().iter().collect(),
                other => vec![other],
            };
            for statement in inner {
                let span = statement.span();
                kept.push(&content[span.start.offset as usize..span.end.offset as usize]);
            }
        }
        kept
    }

    fn keeps(content: &str, statement: &str) -> bool {
        let kept = kept_statements(content);
        let found = kept.iter().any(|kept| kept.starts_with(statement));
        if !found {
            eprintln!("{statement:?} not in {kept:?}");
        }
        found
    }

    fn routes(last: &str) -> String {
        format!(
            "<?php\nnamespace App;\nuse App\\Http\\Controllers\\HomeController;\nRoute::get('/a', fn () => 1);\n{last}"
        )
    }

    #[test]
    fn a_statement_the_end_of_the_file_cuts_off_is_kept() {
        for last in [
            "Route::get('/', [HomeController::class, 'index']\n",
            "Route::get('/', [HomeController::class, 'index'\n",
            "Route::get('/', [HomeController::class, 'index'])\n",
            "Route::get('/', [HomeController::class, 'index']  // home\n",
            "Route::get('/', [HomeController::class, 'index']",
            "Route::get('/', [HomeController::class, 'index'])",
        ] {
            assert!(keeps(&routes(last), "Route::get('/',"), "{last:?}");
        }
        assert!(keeps(
            "<?php\nuse App\\Http\\Controllers\\HomeController;\nRoute::get('/', [HomeController::class, 'index']\n",
            "Route::get('/',"
        ));
    }

    #[test]
    fn a_call_cut_off_before_its_first_argument_is_kept() {
        let head = "<?php\nnamespace App;\nuse App\\Services\\Mailer;\n";
        for last in [
            "$mailer = new Mailer(\n",
            "$mailer = new Mailer(",
            "$mailer = new Mailer(1, ",
            "$mailer = foo(bar(\n",
            "$mailer = foo([\n",
        ] {
            assert!(keeps(&format!("{head}{last}"), last.trim_end()), "{last:?}");
        }
    }

    #[test]
    fn a_static_access_cut_off_before_its_member_is_kept() {
        let head = "<?php\nnamespace App;\nuse App\\Services\\Mailer;\n";
        for last in [
            "$mailer = Mailer::\n",
            "$mailer = Mailer::",
            "foo(Mailer::\n",
            "foo(Mailer::",
        ] {
            assert!(keeps(&format!("{head}{last}"), last.trim_end()), "{last:?}");
        }
    }

    #[test]
    fn a_body_the_end_of_the_file_cuts_off_is_kept() {
        assert!(keeps(
            "<?php\nfunction f() {\n    foo(1\n\n\n\n",
            "function f"
        ));
        assert!(keeps(
            "<?php\nnamespace App {\nclass A { function f() { new Mailer(",
            "class A"
        ));
    }

    #[test]
    fn brackets_before_a_brace_are_closed_in_a_file_the_end_cuts_off() {
        let content = concat!(
            "<?php\n",
            "class Shop {\n",
            "    public function make() {\n",
            "        $w = new Widget(\n",
            "    }\n",
            "}\n",
            "$x = new Gadget(",
        );
        let class_end = content.find("}\n$x").unwrap() + 1;
        assert!(kept_statements(content).contains(&&content[6..class_end]));
        assert!(keeps(content, "$x = new Gadget("));
    }
}
