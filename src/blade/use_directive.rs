//! Scanner for the `@use(...)` directive in a template's own text.
//!
//! `@use('App\Models\Post')` is Blade's way of importing a name into a
//! template: it compiles to a PHP `use` statement.  A PHP import is only
//! valid at the top level, and the preprocessor wraps the template body in
//! a function, so it hoists the directive into the virtual prologue as a
//! real `use` statement.  That prologue has no template text behind it,
//! and anything the virtual file records there translates back through the
//! source map to no position at all.  A feature that needs the directive's
//! position in the template (rewriting the imported name, say, or reporting
//! an import nothing uses) therefore reads the raw template text with the
//! functions here instead of going through the lowered PHP.

use std::ops::Range;

use super::directives::{DirectiveHead, directive_head};
use super::preprocessor::build_use_statement;
use super::signature;
use crate::diagnostics::use_statements::find_use_member;

/// One `@use(...)` directive in a template's text.
pub(crate) struct UseDirective<'a> {
    /// The byte range of the whole directive, from its `@` through the `)`
    /// that closes its argument list.
    pub(crate) span: Range<usize>,
    /// The byte offset of the argument list, the parentheses excluded.
    pub(crate) arguments_at: usize,
    /// The text of the argument list, the parentheses excluded.
    pub(crate) arguments: &'a str,
}

/// Every `@use(...)` directive in `content`.
///
/// Scans the [`signature::inert_regions`]-masked text so a `@use` inside a
/// Blade comment or a `@php` block reads as inert text rather than a real
/// directive, and requires [`directives::directive_head`]'s word-boundary
/// check so a name merely ending in `use` (or `@@use`, its escape) is not
/// mistaken for the directive either.
pub(crate) fn use_directives(content: &str) -> impl Iterator<Item = UseDirective<'_>> {
    let masked = signature::mask_inert_regions(content, true);
    let mut searched = 0;
    std::iter::from_fn(move || {
        loop {
            let at = searched + masked[searched..].find('@')?;
            let bytes = masked.as_bytes();
            let DirectiveHead::Named {
                name, open, args, ..
            } = directive_head(&masked, bytes, at, bytes.len())
            else {
                searched = at + 1;
                continue;
            };
            let Some(args) = args else {
                searched = at + 1;
                continue;
            };
            searched = args.end;
            if name != "use" {
                continue;
            }
            return Some(UseDirective {
                span: at..args.end,
                arguments_at: open + 1,
                arguments: &content[open + 1..args.end - 1],
            });
        }
    })
}

/// Where `template` imports `fqn` as `alias` with a `@use` directive: the
/// byte range of the whole directive, or of the one member that imports it
/// when the directive is a group import naming several.
///
/// A directive matches by the `use` statement the preprocessor hoists it
/// into, so whichever form it is written in, it answers for the same import
/// the virtual PHP's import table holds.
pub(crate) fn find_use_directive(template: &str, fqn: &str, alias: &str) -> Option<Range<usize>> {
    use_directives(template).find_map(|directive| {
        let statement = build_use_statement(directive.arguments)?;
        let member = find_use_member(statement.trim_end_matches(';'), fqn, alias)?;
        if member.member_count < 2 {
            return Some(directive.span);
        }
        // A group import cannot take the two-argument alias form, so the
        // statement is the literal behind a `use` keyword, and the member
        // found in it is found at the same place in the literal.
        const KEYWORD: &str = "use ";
        let (literal_at, literal) = first_string_literal(directive.arguments)?;
        let written = format!("{KEYWORD}{literal}");
        let member = find_use_member(&written, fqn, alias)?;
        let at = directive.arguments_at + literal_at - KEYWORD.len();
        Some(at + member.start..at + member.end)
    })
}

/// The first quoted string in an argument list, as its byte offset within
/// the list and its text with the quotes stripped.
///
/// `@use` takes the imported name first and an optional alias second, so
/// the first string is the only one that names a class.
pub(crate) fn first_string_literal(arguments: &str) -> Option<(usize, &str)> {
    let open = arguments.find(['\'', '"'])?;
    let quote = arguments.as_bytes()[open];
    let close = open + 1 + arguments[open + 1..].find(quote as char)?;
    Some((open + 1, &arguments[open + 1..close]))
}

/// The name a `@use` literal imports, as its byte offset within the
/// literal and its text.
///
/// Strips the `function` / `const` modifier and an inline `as` alias, and
/// for a group import answers the shared prefix rather than the braces.
pub(crate) fn imported_name(literal: &str) -> Option<(usize, &str)> {
    let mut at = literal.len() - literal.trim_start().len();
    let mut rest = &literal[at..];

    for modifier in ["function ", "const "] {
        if let Some(stripped) = rest.strip_prefix(modifier) {
            let trimmed = stripped.trim_start();
            at += modifier.len() + (stripped.len() - trimmed.len());
            rest = trimmed;
            break;
        }
    }

    let end = rest
        .find(" as ")
        .or_else(|| rest.find('{'))
        .unwrap_or(rest.len());
    let name = rest[..end].trim_end().trim_end_matches('\\');
    (!name.is_empty()).then_some((at, name))
}

/// The members of a group import (`'App\Models\{Post, Comment}'`), as the
/// byte offset of the braced list within the literal and each member's
/// offset within that list.
///
/// `None` when the literal is not a group import.
pub(crate) fn group_members(literal: &str) -> Option<(usize, Vec<(usize, &str)>)> {
    let open = literal.find('{')?;
    let close = open + literal[open..].find('}')?;
    let list = &literal[open + 1..close];

    let mut members = Vec::new();
    let mut at = 0;
    for member in list.split(',') {
        let name = member.trim();
        if !name.is_empty() {
            members.push((at + (member.len() - member.trim_start().len()), name));
        }
        at += member.len() + 1;
    }
    Some((open + 1, members))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `@@use` is an escaped directive Blade prints as text, `@used` is a
    /// different word, and a bare `@use` with no argument list imports
    /// nothing; only the real directive is found, with its own span and the
    /// offset of its argument list.
    #[test]
    fn only_a_real_use_directive_yields_its_argument_list() {
        let template = "@@use('A')\n@used('B')\n@use\n@use ('C', 'D')\n";
        let found: Vec<UseDirective> = use_directives(template).collect();
        assert_eq!(found.len(), 1);
        assert_eq!(&template[found[0].span.clone()], "@use ('C', 'D')");
        assert_eq!(found[0].arguments, "'C', 'D'");
        assert_eq!(found[0].arguments_at, 33);
        assert_eq!(&template[33..33 + "'C', 'D'".len()], "'C', 'D'");
    }

    /// A `@use` inside a Blade comment is inert text, not an import, the
    /// same as it is inside a `@php` block.
    #[test]
    fn a_commented_out_or_php_block_use_directive_is_not_an_import() {
        let template =
            "{{-- @use('App\\Foo') --}}\n@php\n@use('App\\Bar')\n@endphp\n@use('App\\Baz')\n";
        let found: Vec<&str> = use_directives(template)
            .map(|directive| directive.arguments)
            .collect();
        assert_eq!(found, vec!["'App\\Baz'"]);
    }

    /// Every form a directive imports in is found by the import it makes:
    /// the whole directive for a single name, and the one member for a
    /// group that names several.
    #[test]
    fn find_use_directive_answers_for_the_import_the_directive_makes() {
        let template = "@use('App\\Models\\Post')\n\
                        @use('App\\Models\\Tag', 'Label')\n\
                        @use('\\App\\Models\\Comment as Remark')\n\
                        @use('function App\\Support\\helper')\n\
                        @use('App\\Models\\{User, Team as Squad}')\n\
                        @use('App\\Models\\{Only}')\n";
        let found = |fqn: &str, alias: &str| {
            find_use_directive(template, fqn, alias).map(|span| &template[span])
        };

        assert_eq!(
            found("App\\Models\\Post", "Post"),
            Some("@use('App\\Models\\Post')")
        );
        assert_eq!(
            found("App\\Models\\Tag", "Label"),
            Some("@use('App\\Models\\Tag', 'Label')")
        );
        assert_eq!(
            found("App\\Models\\Comment", "Remark"),
            Some("@use('\\App\\Models\\Comment as Remark')")
        );
        assert_eq!(
            found("App\\Support\\helper", "helper"),
            Some("@use('function App\\Support\\helper')")
        );
        assert_eq!(found("App\\Models\\User", "User"), Some("User"));
        assert_eq!(found("App\\Models\\Team", "Squad"), Some("Team as Squad"));
        // A group of one has nothing left to keep.
        assert_eq!(
            found("App\\Models\\Only", "Only"),
            Some("@use('App\\Models\\{Only}')")
        );
        // An alias is the name the import binds, so the class's own short
        // name does not match an aliased import of it.
        assert_eq!(found("App\\Models\\Tag", "Tag"), None);
        assert_eq!(found("App\\Models\\Missing", "Missing"), None);
    }

    /// The modifier and an inline alias are not part of the name, and the
    /// offset points at where the name itself starts.
    #[test]
    fn imported_name_strips_the_modifier_and_an_inline_alias() {
        assert_eq!(
            imported_name("function  App\\helper"),
            Some((10, "App\\helper"))
        );
        assert_eq!(imported_name("const App\\LIMIT"), Some((6, "App\\LIMIT")));
        assert_eq!(
            imported_name("App\\Models\\Post as Article"),
            Some((0, "App\\Models\\Post"))
        );
        assert_eq!(
            imported_name(" App\\Models\\{Post, Comment}"),
            Some((1, "App\\Models"))
        );
        assert_eq!(imported_name("   "), None);
    }

    /// Each member is reported at its own offset within the braced list,
    /// with the surrounding whitespace excluded.
    #[test]
    fn group_members_reports_each_member_at_its_offset_in_the_list() {
        let literal = "App\\Models\\{Post,  Comment , }";
        let (list_at, members) = group_members(literal).unwrap();
        assert_eq!(list_at, 12);
        assert_eq!(members, vec![(0, "Post"), (7, "Comment")]);
        for (at, member) in members {
            assert_eq!(&literal[list_at + at..list_at + at + member.len()], member);
        }
        assert_eq!(group_members("App\\Models\\Post"), None);
    }
}
