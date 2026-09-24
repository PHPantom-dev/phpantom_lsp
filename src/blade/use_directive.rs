//! Scanner for the `@use(...)` directive in a template's own text.
//!
//! `@use('App\Models\Post')` is Blade's way of importing a name into a
//! template: it compiles to a PHP `use` statement.  A PHP import is only
//! valid at the top level, and the preprocessor wraps the template body in
//! a function, so it hoists the directive into the virtual prologue as a
//! real `use` statement.  That prologue has no template text behind it,
//! and anything the virtual file records there translates back through the
//! source map to no position at all.  A feature that needs the directive's
//! position in the template (rewriting the imported name, say) therefore
//! reads the raw template text with the functions here instead of going
//! through the lowered PHP.

use super::directives::{DirectiveHead, directive_head};
use super::signature;

/// Every `@use(...)` directive in `content`, as the byte offset of its
/// argument list and the text of it (the parentheses excluded).
///
/// Scans the [`signature::inert_regions`]-masked text so a `@use` inside a
/// Blade comment or a `@php` block reads as inert text rather than a real
/// directive, and requires [`directives::directive_head`]'s word-boundary
/// check so a name merely ending in `use` (or `@@use`, its escape) is not
/// mistaken for the directive either.
pub(crate) fn use_directive_arguments(content: &str) -> impl Iterator<Item = (usize, &str)> {
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
            return Some((open + 1, &content[open + 1..args.end - 1]));
        }
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
    /// nothing; only the real directive is found, at the offset of its
    /// argument list.
    #[test]
    fn only_a_real_use_directive_yields_its_argument_list() {
        let template = "@@use('A')\n@used('B')\n@use\n@use ('C', 'D')\n";
        let found: Vec<(usize, &str)> = use_directive_arguments(template).collect();
        assert_eq!(found, vec![(33, "'C', 'D'")]);
        assert_eq!(&template[33..33 + "'C', 'D'".len()], "'C', 'D'");
    }

    /// A `@use` inside a Blade comment is inert text, not an import, the
    /// same as it is inside a `@php` block.
    #[test]
    fn a_commented_out_or_php_block_use_directive_is_not_an_import() {
        let template =
            "{{-- @use('App\\Foo') --}}\n@php\n@use('App\\Bar')\n@endphp\n@use('App\\Baz')\n";
        let found: Vec<&str> = use_directive_arguments(template)
            .map(|(_, args)| args)
            .collect();
        assert_eq!(found, vec!["'App\\Baz'"]);
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
