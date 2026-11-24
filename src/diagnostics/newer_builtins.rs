//! Uses of a built-in that the project's PHP version does not have yet.
//!
//! The stubs mark what a PHP release added with `@since X.Y`.  Unlike a
//! symbol `@removed` before the target, such a built-in stays resolvable,
//! so the call still has its return type and the user gets this one
//! diagnostic rather than "not found" and a cascade after it.  Only the
//! stub's own declaration is reported: a polyfill declares the name in
//! project or vendor code, and resolution prefers it while the target
//! lacks the built-in.
//!
//! Code written to cope with the built-in's absence is left alone: under
//! `function_exists()`, `class_exists()`, `method_exists()` or `defined()`
//! naming it, or under a check of `PHP_VERSION_ID` and its siblings.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::stubs::StubSymbolKind;
use crate::symbol_map::{ClassRefContext, DocblockMemberRef, SymbolKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{AccessKind, PhpVersion};
use crate::virtual_members::resolve_class_fully_cached;

use super::deprecated::{kind_word, resolve_subject_to_class_name, resolve_variable_subject};
use super::existence_guards::{ExistenceGuards, compute_existence_guards};
use super::helpers::{ByteRange, FileDiagnosticContext, make_diagnostic};

/// The code of a use of a built-in newer than the target PHP version.
pub(crate) const REQUIRES_NEWER_PHP_CODE: &str = "requires_newer_php";

impl Backend {
    /// Report every use of a function, class, method, or constant that a
    /// PHP version newer than the project's added.
    pub fn collect_newer_builtin_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let ctx = FileDiagnosticContext::gather(self, uri);
        self.collect_newer_builtin_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// Same as [`Self::collect_newer_builtin_diagnostics`], reusing an
    /// already-gathered [`FileDiagnosticContext`].
    pub(crate) fn collect_newer_builtin_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if self.newer_stubs.read().is_empty() {
            return;
        }
        let symbol_map = &ctx.symbol_map;
        let Some(source) = symbol_map.source(content) else {
            return;
        };
        let target = self.php_version();
        let _parse_guard = crate::parser::with_parse_cache(content);
        let mut guards: Option<ExistenceGuards> = None;
        let mut comments: Option<Vec<ByteRange>> = None;

        for span in &symbol_map.spans {
            if !super::incremental::reports_site(span.start) {
                continue;
            }
            let found = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context,
                } => {
                    if !names_class_in_code(*context) {
                        continue;
                    }
                    let fqn = if *is_fqn {
                        name.to_string()
                    } else {
                        ctx.file.resolve_name_at(name, span.start)
                    };
                    let fqn = fqn.trim_start_matches('\\');
                    // `Foo::class` is only the name, and loads nothing.
                    if content.get(span.end as usize..).is_some_and(|rest| {
                        rest.trim_start()
                            .strip_prefix("::")
                            .and_then(|rest| rest.trim_start().get(..5))
                            .is_some_and(|word| word.eq_ignore_ascii_case("class"))
                    }) {
                        continue;
                    }
                    if self
                        .newer_stub_version(StubSymbolKind::Class, fqn)
                        .is_none()
                    {
                        continue;
                    }
                    // A type in a docblock describes the code; it does not
                    // load the class.
                    let comments = comments.get_or_insert_with(|| comment_ranges(content));
                    if super::helpers::is_offset_in_ranges(span.start, comments) {
                        continue;
                    }
                    let Some((kind, since)) = self.find_or_load_class(fqn).and_then(|cls| {
                        newer_than(cls.since, target).map(|since| (kind_word(cls.kind), since))
                    }) else {
                        continue;
                    };
                    let guards = guards.get_or_insert_with(|| compute_existence_guards(content));
                    if guards.is_class_guarded(fqn, span.start)
                        || guards.is_class_guarded(name, span.start)
                    {
                        continue;
                    }
                    Some((format!("{kind} {fqn}"), since))
                }

                SymbolKind::MemberAccess {
                    subject_text,
                    member_name,
                    is_static,
                    is_method_call,
                    docblock_ref,
                    ..
                } => {
                    if *docblock_ref != DocblockMemberRef::No
                        || !self.newer_stubs.read().has_member_named(member_name)
                    {
                        continue;
                    }
                    let Some((owner, member)) = self.newer_member(
                        ctx,
                        content,
                        subject_text.as_str(source),
                        member_name,
                        *is_static,
                        *is_method_call,
                        span.start,
                    ) else {
                        continue;
                    };
                    let Some(since) = newer_than(member.since(), target) else {
                        continue;
                    };
                    let guards = guards.get_or_insert_with(|| compute_existence_guards(content));
                    if guards.is_method_guarded(member_name, span.start) {
                        continue;
                    }
                    let subject = match member {
                        Member::Method(_) => format!("Method {owner}::{member_name}()"),
                        Member::Constant(_) => format!("Constant {owner}::{member_name}"),
                    };
                    Some((subject, since))
                }

                SymbolKind::FunctionCall {
                    name,
                    is_definition: false,
                    ..
                } => {
                    let written = name.trim_start_matches('\\');
                    if self
                        .newer_stub_version(StubSymbolKind::Function, written)
                        .or_else(|| {
                            self.newer_stub_version(
                                StubSymbolKind::Function,
                                crate::util::short_name(written),
                            )
                        })
                        .is_none()
                    {
                        continue;
                    }
                    let Some(function) = self.resolve_function_name_at(
                        name,
                        ctx.file.resolved_names.as_deref(),
                        span.start,
                        ctx.file.use_map_at(span.start),
                        &ctx.file.symbol_imports_at(span.start).functions,
                        ctx.file.namespace_at(span.start),
                    ) else {
                        continue;
                    };
                    let Some(since) = newer_than(function.since, target) else {
                        continue;
                    };
                    let guards = guards.get_or_insert_with(|| compute_existence_guards(content));
                    if guards.is_function_guarded(written, span.start)
                        || guards.is_function_guarded(&function.name, span.start)
                    {
                        continue;
                    }
                    let function = match function.namespace.as_deref() {
                        Some(ns) if !ns.is_empty() => format!("{ns}\\{}", function.name),
                        _ => function.name.to_string(),
                    };
                    Some((format!("Function {function}()"), since))
                }

                SymbolKind::ConstantReference {
                    name,
                    is_definition: false,
                } => {
                    let Some((constant, since)) = self.newer_global_constant(ctx, name, span.start)
                    else {
                        continue;
                    };
                    let guards = guards.get_or_insert_with(|| compute_existence_guards(content));
                    if guards.is_constant_guarded(&constant, span.start) {
                        continue;
                    }
                    Some((format!("Constant {constant}"), since))
                }

                _ => None,
            };
            let Some((subject, since)) = found else {
                continue;
            };
            if guards
                .get_or_insert_with(|| compute_existence_guards(content))
                .is_version_guarded(span.start)
            {
                continue;
            }
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Error,
                REQUIRES_NEWER_PHP_CODE,
                format!("{subject} requires PHP {since}, the project targets PHP {target}"),
            ));
        }
    }

    /// The class a member access reads and the method or class constant it
    /// names there, when it names one.
    #[allow(clippy::too_many_arguments)]
    fn newer_member(
        &self,
        ctx: &FileDiagnosticContext,
        content: &str,
        subject: &str,
        member_name: &str,
        is_static: bool,
        is_method_call: bool,
        offset: u32,
    ) -> Option<(String, Member)> {
        let loaders = self.file_loaders(&ctx.file);
        let class_loader = loaders.resolved_class_loader();
        let local_classes = &ctx.file.classes;
        let subject_ctx = crate::type_engine::subject_resolution::SubjectResolutionCtx {
            local_classes,
            use_map: ctx.file.use_map_at(offset),
            namespace: ctx.file.namespace_at(offset),
            content,
            class_loader,
            source_class_loader: loaders.class_loader_at(offset),
            backend: Some(self),
            function_loader: loaders.function_loader_at(offset),
        };
        let base = match resolve_subject_to_class_name(subject, is_static, offset, &subject_ctx)
            .and_then(|name| self.find_or_load_class(&name))
        {
            Some(base) => base,
            None if subject.starts_with('$') => {
                let rctx = ResolutionCtx {
                    is_in_static_method: ctx.symbol_map.is_in_static_method(offset),
                    ..loaders.resolution_ctx_at(content, offset)
                };
                resolve_variable_subject(subject, AccessKind::of_member_access(is_static), &rctx)?
            }
            None => return None,
        };
        let resolved = resolve_class_fully_cached(&base, class_loader, &self.resolved_class_cache);
        let member = if is_method_call {
            base.get_method(member_name)
                .or_else(|| resolved.get_method(member_name))
                .map(|method| Member::Method(method.since))
        } else if is_static {
            resolved
                .constants
                .iter()
                .find(|c| c.name == *member_name)
                .map(|constant| Member::Constant(constant.since))
        } else {
            None
        }?;
        Some((resolved.fqn().to_string(), member))
    }

    /// The fully-qualified name of the built-in constant a reference
    /// names and the version that added it, when the target does not have
    /// it yet and no project or vendor code declares it.
    fn newer_global_constant(
        &self,
        ctx: &FileDiagnosticContext,
        name: &str,
        offset: u32,
    ) -> Option<(String, PhpVersion)> {
        let written = name.trim_start_matches('\\');
        let since = self.newer_stub_version(StubSymbolKind::Constant, written)?;
        // An unqualified name in a namespace is that namespace's constant
        // when one is declared.
        let namespaced = (!name.starts_with('\\') && !written.contains('\\'))
            .then(|| ctx.file.namespace_at(offset).as_deref())
            .flatten()
            .map(|ns| format!("{ns}\\{written}"));
        let candidates: Vec<&str> = namespaced
            .as_deref()
            .into_iter()
            .chain(std::iter::once(written))
            .collect();
        let declared_in_code = self
            .find_or_load_define(
                &candidates,
                crate::resolution::DefineSearch::Exhaustive,
                |info| !crate::stubs::is_embedded_stub_uri(&info.file_uri),
            )
            .unwrap_or(false);
        (!declared_in_code).then(|| (written.to_string(), since))
    }
}

/// A method or class constant, with the version its stub says added it.
enum Member {
    Method(Option<PhpVersion>),
    Constant(Option<PhpVersion>),
}

impl Member {
    fn since(&self) -> Option<PhpVersion> {
        match *self {
            Member::Method(since) | Member::Constant(since) => since,
        }
    }
}

/// `since` when it is newer than `target`.
fn newer_than(since: Option<PhpVersion>, target: PhpVersion) -> Option<PhpVersion> {
    since.filter(|&since| since > target)
}

/// Whether a class reference in `context` is one PHP loads the class for
/// (or may, as a type hint does).
///
/// An import only names the class, and a docblock tag or a string only
/// mentions it.  `catch` and `instanceof` never load it, so naming a class
/// the running PHP lacks there is how code supports both versions.  So is
/// an attribute: one added later (`#[\ReturnTypeWillChange]`,
/// `#[\Override]`) is written to be ignored by the versions before it.
fn names_class_in_code(context: ClassRefContext) -> bool {
    !matches!(
        context,
        ClassRefContext::UseImport
            | ClassRefContext::Catch
            | ClassRefContext::Instanceof
            | ClassRefContext::Attribute
            | ClassRefContext::CoversTarget
            | ClassRefContext::TypeOperatorOperand
            | ClassRefContext::Dql
            | ClassRefContext::DocblockSee
            | ClassRefContext::Annotation
    )
}

/// The byte ranges of the comments in `content`.
fn comment_ranges(content: &str) -> Vec<ByteRange> {
    use mago_syntax::cst::TriviaKind;
    crate::parser::with_parsed_program(content, "comment_ranges", |program, _| {
        program
            .trivia
            .iter()
            .filter(|trivia| trivia.kind != TriviaKind::WhiteSpace)
            .map(|trivia| {
                (
                    trivia.span.start.offset as usize,
                    trivia.span.end.offset as usize,
                )
            })
            .collect()
    })
}
