//! Exceptions a function or method lets escape without documenting them.
//!
//! PhpStorm's "Unhandled exception" inspection and PHPStan's
//! `missingType.checkedException`: a `throw`, or a call whose callee
//! declares `@throws`, raises an exception that no enclosing `try` catches
//! and that no `@throws` tag of the function the code is written in
//! covers.  A tag covers its class and every class extending it.
//!
//! Exceptions extending a class in the `unchecked-exceptions` setting
//! (`Error`, `LogicException` and `RuntimeException` by default) are left
//! alone, and so is one whose class cannot be loaded, since nothing says
//! whether it is checked.  Off unless `unhandled-exceptions` is enabled:
//! it is a documentation contract, not a runtime failure.
//!
//! Code in a closure, an arrow function, or at the top level of a file
//! has no `@throws` of its own to check against, so only named functions
//! and methods are judged.

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::completion::source::throws_analysis::{ThrowsContext, uncaught_throw_sites};
use crate::php_type::PhpType;

use super::helpers::{FileDiagnosticContext, make_diagnostic};

pub(crate) const UNHANDLED_EXCEPTION_CODE: &str = "unhandled_exception";

/// The message prefix the quick fix reads the exception's name back from.
const MESSAGE_PREFIX: &str = "Unhandled exception ";

/// The exception class an `unhandled_exception` diagnostic's message
/// names, fully qualified.
pub(crate) fn unhandled_exception_name(message: &str) -> Option<&str> {
    let rest = message.strip_prefix(MESSAGE_PREFIX)?;
    let name = &rest[..rest.find(':')?];
    (!name.is_empty()).then_some(name)
}

/// A named function or method with a body, and the docblock above it.
struct FunctionLike<'ast, 'arena> {
    /// Where the declaration starts.
    decl_offset: u32,
    docblock: Option<&'ast str>,
    body: &'ast Block<'arena>,
}

struct CollectFunctionLikes<'c> {
    content: &'c str,
}

/// What [`CollectFunctionLikes`] gathers.
struct Found<'ast, 'arena> {
    trivia: &'ast [Trivia<'arena>],
    functions: Vec<FunctionLike<'ast, 'arena>>,
}

impl<'ast, 'arena> Found<'ast, 'arena> {
    fn push(&mut self, content: &str, node: &impl HasSpan, body: &'ast Block<'arena>) {
        let docblock =
            crate::symbol_map::docblock::get_docblock_text_with_offset(self.trivia, content, node)
                .map(|(text, _)| text);
        self.functions.push(FunctionLike {
            decl_offset: node.span().start.offset,
            docblock,
            body,
        });
    }
}

impl<'ast, 'arena> Walker<'ast, 'arena, Found<'ast, 'arena>> for CollectFunctionLikes<'_> {
    fn walk_in_function(&self, function: &'ast Function<'arena>, found: &mut Found<'ast, 'arena>) {
        found.push(self.content, function, &function.body);
    }

    fn walk_in_method(&self, method: &'ast Method<'arena>, found: &mut Found<'ast, 'arena>) {
        if let MethodBody::Concrete(body) = &method.body {
            found.push(self.content, method, body);
        }
    }
}

impl Backend {
    pub(crate) fn collect_unhandled_exception_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let config = self.config();
        if !config.diagnostics.unhandled_exceptions_enabled() || self.is_php_template(uri) {
            return;
        }
        let unchecked = config.diagnostics.unchecked_exceptions();
        let loaders = self.file_loaders(&ctx.file);
        let resolved_class_loader = loaders.resolved_class_loader();

        let _parse_guard = crate::parser::with_parse_cache(content);
        crate::parser::with_parsed_program(content, "unhandled_exceptions", |program, content| {
            let mut found = Found {
                trivia: program.trivia.as_slice(),
                functions: Vec::new(),
            };
            let collector = CollectFunctionLikes { content };
            for statement in program.statements.iter() {
                collector.walk_statement(statement, &mut found);
            }

            for function in &found.functions {
                let body = function.body.span();
                if !super::incremental::reports_site(body.start.offset)
                    && !super::incremental::reports_site(body.end.offset.saturating_sub(1))
                {
                    continue;
                }
                let offset = function.decl_offset;
                let class_loader = loaders.class_loader_at(offset);
                let use_map = ctx.file.use_map_at(offset);
                let throws_ctx = ThrowsContext {
                    class_loader,
                    resolved_class_loader,
                    function_loader: Some(loaders.function_loader_at(offset)),
                    use_map,
                    local_classes: &ctx.file.classes,
                    backend: Some(self),
                };
                let sites = uncaught_throw_sites(function.body, content, found.trivia, &throws_ctx);
                if sites.is_empty() {
                    continue;
                }

                let documented: Vec<String> = function
                    .docblock
                    .map(documented_throws)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|name| {
                        let resolved = crate::names::resolve_code_class_name(
                            &name,
                            ctx.file.namespace_at(offset).as_deref(),
                            Some(use_map),
                            &ctx.file.classes,
                            class_loader,
                        );
                        resolved.trim_start_matches('\\').to_string()
                    })
                    .collect();

                for site in sites {
                    if !super::incremental::reports_site(site.start) {
                        continue;
                    }
                    if !must_document(
                        &site.exception,
                        &documented,
                        &unchecked,
                        resolved_class_loader,
                    ) {
                        continue;
                    }
                    let Some(range) = self.offset_range_to_lsp_range(
                        uri,
                        content,
                        site.start as usize,
                        site.end as usize,
                    ) else {
                        continue;
                    };
                    out.push(make_diagnostic(
                        range,
                        DiagnosticSeverity::Hint,
                        UNHANDLED_EXCEPTION_CODE,
                        format!(
                            "{MESSAGE_PREFIX}{}: neither caught nor declared with @throws",
                            site.exception
                        ),
                    ));
                }
            }
        });
    }
}

/// The class names the `@throws` tags of `docblock` list, as written.
fn documented_throws(docblock: &str) -> Vec<String> {
    let Some(info) = crate::docblock::parser::parse_docblock_for_tags(docblock) else {
        return Vec::new();
    };
    info.tags_by_kind(crate::docblock::TagKind::Throws)
        .flat_map(|tag| {
            let (token, _) =
                crate::docblock::type_strings::split_type_token(tag.description.trim());
            PhpType::parse(token)
                .union_members()
                .into_iter()
                .filter_map(|member| match member.kind() {
                    crate::php_type::TypeKind::Named(name) => Some(name.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether `exception` is a checked exception none of the `documented`
/// classes covers.
fn must_document(
    exception: &str,
    documented: &[String],
    unchecked: &[&str],
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    let Some(class) = class_loader(Fqn::new(exception)) else {
        return false;
    };
    let extends = |ancestor: &str| {
        class.fqn().eq_ignore_ascii_case(ancestor)
            || crate::class_lookup::is_subtype_of(&class, ancestor, class_loader)
    };
    !unchecked.iter().any(|name| extends(name)) && !documented.iter().any(|name| extends(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_exception_back_from_the_message() {
        assert_eq!(
            unhandled_exception_name(
                "Unhandled exception App\\Error\\Failed: neither caught nor declared with @throws"
            ),
            Some("App\\Error\\Failed")
        );
        assert_eq!(unhandled_exception_name("Something else"), None);
    }

    #[test]
    fn reads_every_class_a_throws_tag_lists() {
        let docblock =
            "/**\n * @throws FooException|\\Bar\\BazException when it fails\n * @throws Qux\n */";
        assert_eq!(
            documented_throws(docblock),
            vec!["FooException", "\\Bar\\BazException", "Qux"]
        );
    }
}
