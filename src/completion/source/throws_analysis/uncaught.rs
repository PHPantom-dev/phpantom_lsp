//! Which exceptions a function or method lets escape: everything a `throw`
//! in its body throws, everything a callee it calls declares with
//! `@throws`, and every inline `/** @throws */` annotation, less what an
//! enclosing `try` catches.
//!
//! The body is read from the AST (see [`walk_throw_sites`]), so a string, a
//! comment or a nested closure is never mistaken for code, and receivers
//! and thrown values are typed by the forward walker: `$this->repo->find()`,
//! a variable assigned from a factory, and `throw $e` inside a `catch` all
//! resolve the way hover and diagnostics see them.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;
use tower_lsp::lsp_types::Position;

use crate::atom::{atom, bytes_to_str};
use crate::completion::source::comment_position::{docblock_start_at, position_to_byte_offset};
use crate::php_type::PhpType;
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::type_engine::variable::forward_walk::{self, ThrowSiteSink, walk_throw_sites};
use crate::types::{ClassInfo, FunctionLoader, PerBlock};

/// What the analysis resolves the names and calls in a body with.
pub(crate) struct ThrowsContext<'a> {
    /// Resolves a class name as written in the file to its [`ClassInfo`].
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Resolves a function name to its [`FunctionInfo`](crate::types::FunctionInfo).
    pub function_loader: FunctionLoader<'a>,
    /// Use-statement map for the current file (short name → FQN).
    pub use_map: &'a HashMap<String, String>,
    /// The classes the file declares, which `$this` and `self` name.
    pub local_classes: &'a [Arc<ClassInfo>],
    /// The backend whose caches the type engine reads through.
    pub backend: Option<&'a crate::Backend>,
}

/// [`find_uncaught_throw_types_with_context`] with nothing to resolve
/// names or calls with: only what the body itself spells out.
pub(crate) fn find_uncaught_throw_types(content: &str, position: Position) -> Vec<PhpType> {
    find_uncaught_throw_types_with_context(content, position, None)
}

/// The exception types the function or method documented at `position`
/// throws without catching, as fully-qualified names in the order the body
/// first throws them.
///
/// `position` is in the docblock above the declaration or at its start, or
/// just after a `/**` being typed there.
pub(crate) fn find_uncaught_throw_types_with_context(
    content: &str,
    position: Position,
    ctx: Option<&ThrowsContext<'_>>,
) -> Vec<PhpType> {
    let no_imports = HashMap::new();
    let no_class = |_: &str| None;
    let fallback;
    let ctx = match ctx {
        Some(ctx) => ctx,
        None => {
            fallback = ThrowsContext {
                class_loader: &no_class,
                function_loader: None,
                use_map: &no_imports,
                local_classes: &[],
                backend: None,
            };
            &fallback
        }
    };
    let resolved_class_loader =
        |name: &str| crate::class_lookup::load_resolved_name(name, ctx.class_loader);
    with_documented_function_body(content, position, |body, content, trivia| {
        // Every receiver is typed from one walk of the body (see
        // `record_scopes`), and a callee's inferred return type is worked
        // out once however often the body calls it.
        let _engine_caches = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _scope_cache = forward_walk::with_diagnostic_scope_cache();
        let mut analysis = Analysis {
            content,
            ctx,
            resolved_class_loader: &resolved_class_loader,
            placeholder: crate::class_lookup::class_context_placeholder(
                content,
                body.span().start.offset,
            ),
            sites: Vec::new(),
            tries: Vec::new(),
        };
        analysis.record_scopes(body);
        for statement in body.statements.iter() {
            walk_throw_sites(statement, &mut analysis);
        }
        analysis.inline_annotations(body.span(), trivia);
        analysis.uncaught()
    })
    .unwrap_or_default()
}

/// Run `with` on the body of the function or method documented at
/// `position`: the one whose docblock holds it, or the one a `/**` being
/// typed at it is about to document.  `None` for an abstract method, or
/// when no declaration follows.
pub(crate) fn with_documented_function_body<T>(
    content: &str,
    position: Position,
    with: impl FnOnce(&Block<'_>, &str, &[Trivia<'_>]) -> T,
) -> Option<T> {
    let offset = position_to_byte_offset(content, position).min(content.len());
    let content = with_open_docblock_blanked(content, offset);
    // The body's receivers are typed by walks of the same text, which then
    // share this parse.
    let _parse_guard = crate::parser::with_parse_cache(&content);
    crate::parser::with_parsed_program(&content, "throws_analysis", |program, content| {
        let mut found = DocumentedFunction {
            offset: offset as u32,
            content,
            trivia: program.trivia.as_slice(),
            body: None,
        };
        for statement in program.statements.iter() {
            FindDocumented.walk_statement(statement, &mut found);
        }
        let body = found.body.flatten()?;
        Some(with(body, content, program.trivia.as_slice()))
    })
}

/// `content` with the docblock `offset` sits in blanked out when it has not
/// been closed yet, so the declaration after it parses: completion on `/**`
/// fires before the editor has closed the comment.
fn with_open_docblock_blanked(content: &str, offset: usize) -> Cow<'_, str> {
    let Some(open) = docblock_start_at(content, offset) else {
        return Cow::Borrowed(content);
    };
    // The comment runs on to the first `*/`.  A line before that which a
    // docblock does not hold, one that does not start with `*`, is code the
    // comment swallowed: the docblock being typed ends where that code
    // starts.
    let after = &content[offset..];
    let comment = after.find("*/").map_or(after, |close| &after[..close]);
    let mut lines = comment.split_inclusive('\n');
    let mut end = offset + lines.next().map_or(0, str::len);
    let mut swallowed = false;
    for line in lines {
        let text = line.trim_start();
        if !text.is_empty() && !text.starts_with('*') {
            swallowed = true;
            break;
        }
        end += line.len();
    }
    if !swallowed && comment.len() < after.len() {
        return Cow::Borrowed(content);
    }
    let blank: String = content[open..end]
        .bytes()
        .map(|byte| if byte == b'\n' { '\n' } else { ' ' })
        .collect();
    let mut blanked = content.to_string();
    blanked.replace_range(open..end, &blank);
    Cow::Owned(blanked)
}

struct DocumentedFunction<'a, 'ast, 'arena> {
    offset: u32,
    content: &'a str,
    trivia: &'a [Trivia<'arena>],
    /// The declaration found: its body, or `None` for one without.
    body: Option<Option<&'ast Block<'arena>>>,
}

impl<'ast, 'arena> DocumentedFunction<'_, 'ast, 'arena> {
    /// Take `node` when the code at `offset` documents it: `offset` is in
    /// the docblock right above it, or only whitespace lies between them.
    fn consider(&mut self, node: &impl HasSpan, body: Option<&'ast Block<'arena>>) {
        if self.body.is_some() {
            return;
        }
        let start = node.span().start.offset;
        if start < self.offset {
            return;
        }
        let documents = match crate::symbol_map::docblock::get_docblock_text_with_offset(
            self.trivia,
            self.content,
            node,
        ) {
            Some((_, docblock_start)) => docblock_start <= self.offset,
            None => self.content[self.offset as usize..start as usize]
                .trim()
                .is_empty(),
        };
        if documents {
            self.body = Some(body);
        }
    }
}

struct FindDocumented;

impl<'ast, 'arena> Walker<'ast, 'arena, DocumentedFunction<'_, 'ast, 'arena>> for FindDocumented {
    fn walk_in_function(
        &self,
        function: &'ast Function<'arena>,
        found: &mut DocumentedFunction<'_, 'ast, 'arena>,
    ) {
        found.consider(function, Some(&function.body));
    }

    fn walk_in_method(
        &self,
        method: &'ast Method<'arena>,
        found: &mut DocumentedFunction<'_, 'ast, 'arena>,
    ) {
        let body = match &method.body {
            MethodBody::Concrete(block) => Some(block),
            MethodBody::Abstract(_) => None,
        };
        found.consider(method, body);
    }
}

/// A `try` statement's protected block and what its `catch` clauses take.
struct TryRegion {
    block: (u32, u32),
    /// The caught classes, resolved.
    catches: Vec<String>,
}

/// The throw sites of one body, as they are collected.
struct Analysis<'a, 'c> {
    content: &'a str,
    ctx: &'a ThrowsContext<'c>,
    /// Loads a class by the name the analysis resolved it to.
    resolved_class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// What `$this` and `self` are read as outside a class.
    placeholder: ClassInfo,
    /// Each exception class a site can throw, with the site's offset.
    sites: Vec<(u32, String)>,
    tries: Vec<TryRegion>,
}

impl Analysis<'_, '_> {
    /// Walk the body once for the scope each of its statements runs in, so
    /// typing a receiver reads the scope there instead of walking the body
    /// up to it.
    fn record_scopes(&self, body: &Block<'_>) {
        forward_walk::build_diagnostic_scopes_for_offsets(
            self.content,
            self.ctx.local_classes,
            &PerBlock::single(self.resolved_class_loader),
            &PerBlock::single(self.ctx.class_loader),
            &PerBlock::single(self.ctx.use_map),
            self.ctx.backend,
            Loaders::with_function(self.ctx.function_loader),
            self.ctx
                .backend
                .map(|backend| &backend.resolved_class_cache),
            &[body.span().start.offset],
        );
    }

    /// The class the code at `offset` is written in, or the placeholder
    /// for its namespace outside one.
    fn current_class(&self, offset: u32) -> &ClassInfo {
        crate::class_lookup::find_class_at_offset(self.ctx.local_classes, offset)
            .unwrap_or(&self.placeholder)
    }

    /// The forward walker's context for an expression at `offset`.
    fn var_ctx(&self, offset: u32) -> VarResolutionCtx<'_> {
        let current_class = self.current_class(offset);
        VarResolutionCtx {
            source_imports: Some(self.ctx.use_map),
            backend: self.ctx.backend,
            loaders: Loaders::with_function(self.ctx.function_loader),
            resolved_class_cache: self
                .ctx
                .backend
                .map(|backend| &backend.resolved_class_cache),
            ..VarResolutionCtx::new(
                "",
                current_class,
                self.ctx.local_classes,
                self.content,
                offset,
                self.resolved_class_loader,
                self.ctx.class_loader,
            )
        }
    }

    /// The FQN of a class `name` written in the file names, spelled the way
    /// its declaration spells it when it can be loaded.
    fn written_class(&self, name: &str, offset: u32) -> String {
        let resolved = crate::names::resolve_code_class_name(
            name,
            self.current_class(offset).file_namespace.as_deref(),
            Some(self.ctx.use_map),
            self.ctx.local_classes,
            self.ctx.class_loader,
        );
        self.canonical(&resolved)
    }

    fn canonical(&self, fqn: &str) -> String {
        (self.resolved_class_loader)(fqn)
            .map(|cls| cls.fqn().to_string())
            .unwrap_or_else(|| fqn.trim_start_matches('\\').to_string())
    }

    /// The class `expr` names in `new X` or `X::m()`.
    fn class_named(&self, expr: &Expression<'_>, offset: u32) -> Option<String> {
        let var_ctx = self.var_ctx(offset);
        crate::class_lookup::class_expression_name(
            expr,
            var_ctx.current_class,
            self.ctx.local_classes,
            self.ctx.class_loader,
            Some(self.ctx.use_map),
        )
        .map(|name| self.canonical(&name))
    }

    /// The classes `expr` holds where it is written.
    fn classes_of(&self, expr: &Expression<'_>) -> Vec<String> {
        let offset = expr.span().start.offset;
        let Some(ty) = crate::type_engine::variable::foreach_resolution::resolve_expression_type(
            expr,
            &self.var_ctx(offset),
        ) else {
            return Vec::new();
        };
        ty.union_members()
            .into_iter()
            .filter_map(|member| {
                let name = member.non_null_type().unwrap_or_else(|| member.clone());
                let name = name.base_name()?;
                (!crate::php_type::is_primitive_scalar_name(name)).then(|| self.canonical(name))
            })
            .collect()
    }

    fn merged(&self, class_name: &str) -> Option<Arc<ClassInfo>> {
        let cls = (self.resolved_class_loader)(class_name)?;
        Some(crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            self.resolved_class_loader,
            self.ctx
                .backend
                .map(|backend| &backend.resolved_class_cache),
        ))
    }

    /// Add the exceptions a callee declares with `@throws`.
    fn add_declared(&mut self, offset: u32, throws: &[PhpType]) {
        for ty in throws {
            for member in ty.union_members() {
                if let Some(name) = member.base_name() {
                    let fqn = self.canonical(name);
                    self.sites.push((offset, fqn));
                }
            }
        }
    }

    fn add_method(
        &mut self,
        offset: u32,
        classes: Vec<String>,
        method: &ClassLikeMemberSelector<'_>,
    ) {
        let ClassLikeMemberSelector::Identifier(ident) = method else {
            return;
        };
        let method_name = bytes_to_str(ident.value);
        for class_name in classes {
            if let Some(merged) = self.merged(&class_name)
                && let Some(method) = merged.get_method(method_name)
            {
                let throws = method.throws().to_vec();
                self.add_declared(offset, &throws);
            }
        }
    }

    /// Add the exceptions the `/** @throws X */` annotations written inside
    /// the body name, at the annotation.
    fn inline_annotations(&mut self, body: mago_span::Span, trivia: &[Trivia<'_>]) {
        for comment in trivia {
            if comment.kind != TriviaKind::DocBlockComment
                || comment.span.start.offset <= body.start.offset
                || comment.span.end.offset >= body.end.offset
            {
                continue;
            }
            let Some(info) =
                crate::docblock::parser::parse_docblock_for_tags(bytes_to_str(comment.value))
            else {
                continue;
            };
            let offset = comment.span.start.offset;
            for tag in info.tags_by_kind(crate::docblock::TagKind::Throws) {
                let Some(name) = tag.description.split_whitespace().next() else {
                    continue;
                };
                if name.starts_with('$') {
                    continue;
                }
                let fqn = self.written_class(name, offset);
                self.sites.push((offset, fqn));
            }
        }
    }

    /// Whether a `catch` of `catch_fqn` takes `thrown`.
    fn catches(&self, catch_fqn: &str, thrown: &str) -> bool {
        if catch_fqn.eq_ignore_ascii_case(thrown) || catch_fqn.eq_ignore_ascii_case("Throwable") {
            return true;
        }
        match (self.resolved_class_loader)(thrown) {
            Some(thrown_class) => crate::class_lookup::is_subtype_of(
                &thrown_class,
                catch_fqn,
                self.resolved_class_loader,
            ),
            // Nothing is known about an exception class that cannot be
            // loaded, so a catch of the broadest classes is taken to cover
            // it.
            None => catch_fqn.eq_ignore_ascii_case("Exception"),
        }
    }

    /// The exception classes thrown where no enclosing `try` catches them,
    /// first throw first, each once.
    fn uncaught(mut self) -> Vec<PhpType> {
        self.sites.sort_by_key(|(offset, _)| *offset);
        let mut uncaught: Vec<PhpType> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (offset, thrown) in &self.sites {
            let caught = self.tries.iter().any(|region| {
                region.block.0 < *offset
                    && *offset < region.block.1
                    && region
                        .catches
                        .iter()
                        .any(|caught| self.catches(caught, thrown))
            });
            if !caught && seen.insert(thrown.to_ascii_lowercase()) {
                uncaught.push(PhpType::named(atom(thrown)));
            }
        }
        uncaught
    }
}

impl<'ast, 'arena> ThrowSiteSink<'ast, 'arena> for Analysis<'_, '_> {
    fn function_call(&mut self, call: &'ast FunctionCall<'arena>) {
        let Expression::Identifier(ident) = call.function else {
            return;
        };
        let offset = call.span().start.offset;
        if let Some(function_loader) = self.ctx.function_loader
            && let Some(function) = function_loader(bytes_to_str(ident.value()), offset)
        {
            self.add_declared(offset, &function.throws);
        }
    }

    fn method_call(
        &mut self,
        object: &'ast Expression<'arena>,
        method: &'ast ClassLikeMemberSelector<'arena>,
    ) {
        let classes = self.classes_of(object);
        self.add_method(object.span().start.offset, classes, method);
    }

    fn static_method_call(&mut self, call: &'ast StaticMethodCall<'arena>) {
        let offset = call.span().start.offset;
        let classes = match self.class_named(call.class, offset) {
            Some(class) => vec![class],
            None => self.classes_of(call.class),
        };
        self.add_method(offset, classes, &call.method);
    }

    fn instantiation(&mut self, new: &'ast Instantiation<'arena>) {
        let offset = new.span().start.offset;
        if let Some(class) = self.class_named(new.class, offset)
            && let Some(merged) = self.merged(&class)
            && let Some(constructor) = merged.get_method("__construct")
        {
            let throws = constructor.throws().to_vec();
            self.add_declared(offset, &throws);
        }
    }

    fn throw(&mut self, throw: &'ast Throw<'arena>) {
        let offset = throw.span().start.offset;
        let classes = match throw.exception {
            Expression::Instantiation(new) => {
                self.class_named(new.class, offset).into_iter().collect()
            }
            thrown => self.classes_of(thrown),
        };
        for class in classes {
            self.sites.push((offset, class));
        }
    }

    // Anything at all is not something a `@throws` tag can name.
    fn any_exception(&mut self) {}

    fn try_statement(&mut self, try_statement: &'ast Try<'arena>) {
        let block = try_statement.block.span();
        let catches = try_statement
            .catch_clauses
            .iter()
            .flat_map(|clause| {
                let offset = clause.span().start.offset;
                crate::parser::extract_hint_type(&clause.hint)
                    .union_members()
                    .into_iter()
                    .filter_map(|member| match member.kind() {
                        crate::php_type::TypeKind::Named(name) => {
                            Some(self.written_class(name, offset))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        self.tries.push(TryRegion {
            block: (block.start.offset, block.end.offset),
            catches,
        });
    }
}
