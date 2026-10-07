//! Return-type inference from function bodies.
//!
//! Walks the `return` statements of a function via the AST and
//! resolves each returned expression's type through the shared RHS
//! resolution pipeline (the same one hover, completion, and
//! diagnostics use).

use std::sync::Arc;

use mago_span::Span;
use mago_syntax::cst::Block;
use mago_syntax::cst::Statement;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::variable::Variable;
use tower_lsp::lsp_types::Position;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::return_collection::collect_returns;
use crate::text_position::line_start_byte_offset;
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::type_engine::variable::foreach_resolution::resolve_expression_type;
use crate::types::{ClassInfo, FunctionLoader};

use super::edits::find_open_brace_from_declaration;

// ── Return type inference result ────────────────────────────────────────────

/// The result of inferring a return type from a function body.
///
/// Separates the native PHP type hint (for the `: type` declaration)
/// from the effective PHPStan type (for a `@return` docblock tag).
/// When the two are identical, no docblock is needed.
pub(crate) struct InferredReturnType {
    /// Valid native PHP type hint (e.g. `array`, `int`, `Foo`).
    pub(crate) native: PhpType,
    /// Full effective type including generics/shapes (e.g. `list<string>`).
    /// `None` when the native type already captures the full type.
    pub(crate) effective: Option<PhpType>,
    /// Whether every `return` in the body contributed the same type.
    ///
    /// One `return` (or several that agree) leaves nothing for the join to
    /// reconstruct: the answer is whatever that expression resolves to,
    /// nullable or generic or otherwise. Several that disagree make the
    /// result a reconstruction of the control flow instead, and it is only
    /// as complete as the branch analysis behind it — a body whose third
    /// `return` this walk mistyped or dropped still reports the other two
    /// as if they were the whole answer. Callers that have a declared type
    /// to fall back on use this to decide whether the reading is worth
    /// preferring over the declaration.
    pub(crate) returns_agree: bool,
}

// ── Backend methods ─────────────────────────────────────────────────────────

impl Backend {
    /// Infer the return type of the function at `func_line` by scanning
    /// all return statements in the body.
    ///
    /// Returns an [`InferredReturnType`] that separates the native PHP
    /// type hint from the richer effective type.  When they differ (e.g.
    /// `list<string>` vs `array`), the caller should add a `@return` tag.
    ///
    /// When `self_as_marker` is `true`, `return $this;` yields the self-like
    /// marker `$this` so the type engine can map it to the receiver class
    /// rather than the (possibly trait) class that declares the method.
    pub(crate) fn infer_return_type_for_function(
        &self,
        uri: &str,
        content: &str,
        func_line: usize,
        self_as_marker: bool,
    ) -> Option<InferredReturnType> {
        let offset = body_brace_offset(content, func_line)?;
        self.infer_return_type_at(uri, content, offset, self_as_marker)
    }

    /// [`infer_return_type_for_function`](Self::infer_return_type_for_function)
    /// for the function or method whose declaration, from its name to its
    /// closing brace, contains `offset` (its `name_offset` will do).
    pub(crate) fn infer_return_type_at(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
        self_as_marker: bool,
    ) -> Option<InferredReturnType> {
        // Set up the resolution infrastructure from Backend state.
        let local_classes: Vec<Arc<ClassInfo>> = self
            .symbols
            .uri_classes_index
            .read()
            .get(uri)
            .cloned()
            .unwrap_or_default();
        let (file_use_map, file_namespace) = self.use_map_and_namespace_at(uri, offset);
        let class_loader = self.class_loader_with(&local_classes, &file_use_map, &file_namespace);
        let function_loader = self.function_loader_with(None, &file_use_map, &file_namespace);

        infer_return_type_at(
            content,
            offset,
            &local_classes,
            &class_loader,
            Some(self),
            Some(&function_loader),
            self_as_marker,
        )
    }
}

// ── Shared return-type inference ────────────────────────────────────────────

/// Infer the return type of a function by walking all `return`
/// statements reachable from its body (not crossing into nested
/// closures/arrow functions, which have their own return types).
///
/// Every returned expression is resolved through
/// [`resolve_expression_type`] — the same RHS resolution pipeline used
/// by hover, completion, and diagnostics — so literals, array shapes,
/// `new` instantiations, method calls, and variables are all handled
/// consistently and any future improvement to that pipeline benefits
/// this inference automatically.
///
/// Returns an [`InferredReturnType`] that separates the native PHP
/// type hint from the richer effective type.  When they differ (e.g.
/// `list<string>` vs `array`), the caller should add a `@return` tag.
///
/// When `self_as_marker` is `true`, a `return $this;` statement yields
/// the self-like marker `$this` instead of resolving to the concrete
/// enclosing class.  The type engine needs this so that a fluent method
/// inherited from a trait maps to the class the method is *called on*,
/// not the trait that lexically declares it.  Code actions that write a
/// concrete return type or docblock pass `false`.
///
/// This is the shared core used by:
/// - `Backend::infer_return_type_for_function` (PHPStan code actions)
/// - `enrichment_return_type` (Generate / Update PHPDoc)
pub(crate) fn infer_return_type(
    content: &str,
    func_line: usize,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&Backend>,
    function_loader: FunctionLoader<'_>,
    self_as_marker: bool,
) -> Option<InferredReturnType> {
    infer_return_type_at(
        content,
        body_brace_offset(content, func_line)?,
        local_classes,
        class_loader,
        backend,
        function_loader,
        self_as_marker,
    )
}

/// The offset of the opening brace of the declaration on `func_line`, which
/// falls inside the body however it is formatted.
fn body_brace_offset(content: &str, func_line: usize) -> Option<u32> {
    let lines: Vec<&str> = content.lines().collect();
    if func_line >= lines.len() {
        return None;
    }
    let brace_line = find_open_brace_from_declaration(&lines, func_line)?;
    let brace_col = lines[brace_line].find('{')?;
    Some((line_start_byte_offset(content, brace_line) + brace_col) as u32)
}

/// [`infer_return_type`] for the function or method whose declaration,
/// from its name to its closing brace, contains `offset`.
fn infer_return_type_at(
    content: &str,
    offset: u32,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&Backend>,
    function_loader: FunctionLoader<'_>,
    self_as_marker: bool,
) -> Option<InferredReturnType> {
    let enclosing_class = local_classes.iter().find(|c| {
        !c.name.starts_with("__anonymous@") && offset >= c.start_offset && offset <= c.end_offset
    });
    let no_class = ClassInfo::default();
    let enclosing_class: &ClassInfo = enclosing_class.map_or(&no_class, |c| c);

    let (mut return_types, has_bare_return, has_return_with_value) =
        with_parsed_program(content, "fix_return_type_infer", |program, _content| {
            let body = declared_body(program.statements.iter(), offset)?;

            let mut returns: Vec<(Option<&Expression<'_>>, usize, usize, usize)> = Vec::new();
            collect_returns(body.statements.iter(), &mut returns);

            let mut return_types: Vec<PhpType> = Vec::new();
            let mut has_bare_return = false;
            let mut has_return_with_value = false;

            for (maybe_expr, start, _end, _stmt_start) in returns {
                let Some(expr) = maybe_expr else {
                    has_bare_return = true;
                    continue;
                };
                has_return_with_value = true;

                // `return $this;` is a fluent self-return.  Yield the
                // self-like marker so the caller maps it to the actual
                // receiver class rather than the class that lexically
                // declares the method (which for a trait method is the
                // trait, not the using class).
                if self_as_marker && is_this_variable(expr) {
                    return_types.push(PhpType::this());
                    continue;
                }

                let ctx = VarResolutionCtx {
                    backend,
                    loaders: Loaders::with_function(function_loader),
                    ..VarResolutionCtx::new(
                        "",
                        enclosing_class,
                        local_classes,
                        content,
                        start as u32,
                        class_loader,
                    )
                };

                let ty = resolve_expression_type(expr, &ctx).unwrap_or_else(PhpType::mixed);
                let ty = ty.resolve_names(&|name: &str| {
                    if let Some(cls) = class_loader(name) {
                        cls.fqn().to_string()
                    } else {
                        name.to_string()
                    }
                });
                return_types.push(ty);
            }

            Some((return_types, has_bare_return, has_return_with_value))
        })?;

    if !has_return_with_value && !has_bare_return {
        return Some(InferredReturnType {
            native: PhpType::void(),
            effective: None,
            returns_agree: true,
        });
    }

    if return_types.is_empty() && has_bare_return {
        return Some(InferredReturnType {
            native: PhpType::void(),
            effective: None,
            returns_agree: true,
        });
    }

    if has_bare_return {
        return_types.push(PhpType::null());
    }

    let returns_agree = return_types
        .split_first()
        .is_none_or(|(first, rest)| rest.iter().all(|ty| ty.equivalent(first)));

    // Keep exact literal alternatives in the effective PHPDoc type while
    // removing only alternatives made redundant by a broad scalar branch.
    let effective = PhpType::join_runtime_value_types(return_types);

    // The existing inference guard limits unrelated return domains, not the
    // number of exact values within one scalar domain. Otherwise four literal
    // returns would regress from one inferable `string` domain to no code
    // action merely because the shared resolver became more precise.
    let mut complexity_domains: Vec<PhpType> = Vec::new();
    for member in effective.union_members() {
        let domain = match member.as_literal() {
            Some(crate::php_type::LiteralValue::Int(_)) => PhpType::int(),
            Some(crate::php_type::LiteralValue::Float(_)) => PhpType::float(),
            Some(crate::php_type::LiteralValue::String(_)) => PhpType::string(),
            None => member.clone(),
        };
        if !complexity_domains
            .iter()
            .any(|existing| existing.equivalent(&domain))
        {
            complexity_domains.push(domain);
        }
    }
    if complexity_domains.len() > 3 {
        return None;
    }

    // Convert effective type → native PHP type hint. A body that only ever
    // returns `true` is typed `bool` in the signature rather than the
    // standalone `true` type: that spelling needs PHP 8.2, and tightening a
    // method's declared return to one boolean half would reject a subclass
    // that legitimately returns the other. The exact value survives in the
    // `@return` docblock.
    let native = effective
        .widen_boolean_literals()
        .to_native_hint_typed()
        .unwrap_or_else(PhpType::mixed);

    let needs_docblock = !native.equivalent(&effective);
    Some(InferredReturnType {
        native,
        effective: if needs_docblock {
            Some(effective)
        } else {
            None
        },
        returns_agree,
    })
}

/// The body of the function or method among `statements` whose
/// declaration, from its name to its closing brace, contains `offset`.
fn declared_body<'a>(
    statements: impl Iterator<Item = &'a Statement<'a>>,
    offset: u32,
) -> Option<&'a Block<'a>> {
    let declares = |name: &Span, body: &Block<'_>| {
        name.start.offset <= offset && offset <= body.right_brace.end.offset
    };
    for stmt in statements {
        let members = match stmt {
            Statement::Function(func) if declares(&func.name.span, &func.body) => {
                return Some(&func.body);
            }
            Statement::Namespace(ns) => {
                if let Some(body) = declared_body(ns.statements().iter(), offset) {
                    return Some(body);
                }
                continue;
            }
            Statement::Class(class) => class.members.as_slice(),
            Statement::Trait(trait_) => trait_.members.as_slice(),
            Statement::Enum(enum_) => enum_.members.as_slice(),
            _ => continue,
        };
        let mut found = None;
        crate::parser::for_each_concrete_method(members, |method, body| {
            if found.is_none() && declares(&method.name.span, body) {
                found = Some(body);
            }
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

/// Whether `expr` is the bare `$this` variable.
fn is_this_variable(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Variable(Variable::Direct(dv)) if bytes_to_str(dv.name) == "$this")
}

/// Infer a `@return` type string for a function whose signature is
/// at `position` in `content`.
///
/// Returns `Some("list<string>")` when the body analysis produces a
/// type richer than the native hint, or `None` when inference fails
/// or the native type already captures the full information.
///
/// This is the entry point for docblock generation (`enrichment_plain`
/// replacement for `@return`) — it finds the function line from the
/// position and delegates to [`infer_return_type`].
pub(crate) fn enrichment_return_type(
    content: &str,
    position: Position,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&Backend>,
    function_loader: FunctionLoader<'_>,
) -> Option<PhpType> {
    // The position is on or near the docblock / function signature.
    // Search forward from that line to find the `function` keyword.
    let lines: Vec<&str> = content.lines().collect();
    let start = position.line as usize;
    let end = (start + 10).min(lines.len());
    let func_line =
        (start..end).find(|&i| lines[i].contains("function ") || lines[i].contains("function("))?;

    let inferred = infer_return_type(
        content,
        func_line,
        local_classes,
        class_loader,
        backend,
        function_loader,
        // Docblock generation wants a concrete written type, not a `$this`
        // marker, so resolve `return $this` to the enclosing class.
        false,
    )?;

    // Return the effective type if it's richer than the native hint,
    // otherwise return the native type (which may still be useful for
    // callers that want any inferred type, e.g. `void`).
    Some(inferred.effective.unwrap_or(inferred.native))
}
