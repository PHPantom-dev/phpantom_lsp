//! Undefined variable diagnostics.
//!
//! Walk each function/method/closure body in the file and flag every
//! variable read that has no prior definition (assignment, parameter,
//! foreach binding, catch binding, `global`, `static`, `use()` clause,
//! or `list()`/`[…]` destructuring) in the same scope.
//!
//! Diagnostics use `Severity::Error` because accessing an undefined
//! variable is a runtime notice/warning (and `ErrorException` in strict
//! setups).  This is the single most impactful diagnostic for catching
//! typos in variable names.
//!
//! ## Implementation
//!
//! A variable read is flagged only when no write of the same name exists
//! at a **lower byte offset** in the same frame.  Writes inside any
//! control-flow branch (if/else, switch, try/catch) still count, so the
//! analysis is conservative about branches but strict about source order:
//! it catches the common "used before assigned" typo while avoiding false
//! positives from branch-dependent definitions.  `/** @var Type $var */`
//! annotations are treated as a write at the annotation's position and are
//! scoped to the frame they appear in.
//!
//! ## Suppression / false-positive avoidance
//!
//! The following patterns suppress the diagnostic for a variable:
//!
//! - **Superglobals** (`$_GET`, `$_POST`, `$_SERVER`, etc.) and
//!   `$this` are always considered defined.
//! - **`isset($var)` / `empty($var)`** — the variable is being
//!   guarded, not used.  Reads inside `isset()` and `empty()` are
//!   suppressed.
//! - **`compact('var')`** — `$var` is referenced by string name.
//!   All variable names mentioned in `compact()` calls are treated
//!   as defined.
//! - **`extract($array)`** — any variable could be defined; skip the
//!   entire function body.
//! - **`$$dynamic`** — variable variables make static analysis
//!   unsound; skip the entire function body.
//! - **`@$var`** — the error suppression operator signals intentional
//!   use of a potentially undefined variable.
//! - **`unset($var)`** — the variable is being destroyed, not read.
//!   `unset()` itself should not flag the variable.
//! - **`@var` annotation** — a `/** @var Type $var */` comment on
//!   the preceding line means the developer asserts the variable
//!   exists.
//! - **`$this`** inside a non-static method or closure — always
//!   defined.
//! - **`$this`** inside a static method or top-level code — flagged
//!   separately by other tools; we skip it.

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::parser::with_parsed_program;
use crate::scope_collector::{
    AccessKind, ByRefCallKind, ByRefResolver, FrameKind, ScopeMap,
    collect_function_scope_with_kind_and_resolver, collect_function_scope_with_resolver,
};

use super::helpers::make_diagnostic;

/// Diagnostic code used for undefined-variable diagnostics so that
/// code actions can match on it.
pub(crate) const UNKNOWN_VARIABLE_CODE: &str = "unknown_variable";

/// PHP superglobals and auto-defined variables that are always in scope.
const SUPERGLOBALS: &[&str] = &[
    "$_GET",
    "$_POST",
    "$_SERVER",
    "$_REQUEST",
    "$_SESSION",
    "$_COOKIE",
    "$_FILES",
    "$_ENV",
    "$GLOBALS",
    "$argc",
    "$argv",
    "$http_response_header",
    "$php_errormsg",
];

impl Backend {
    /// Collect undefined-variable diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_undefined_variable_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // Gather file-level context for FQN resolution of function and
        // class names inside the by-ref resolver.
        let file_use_map: std::collections::HashMap<String, String> = self.file_use_map(uri);
        let file_namespace: Option<String> = self.first_file_namespace(uri);

        // Build a by-ref resolver that uses Backend to look up function
        // and method signatures.  This lets the scope collector mark
        // by-ref arguments as writes for user-defined functions, static
        // methods, and constructors — not just the hardcoded table.
        let resolver: ByRefResolver<'_> = &|call_kind: &ByRefCallKind<'_>| {
            self.resolve_by_ref_positions(call_kind, &file_use_map, &file_namespace)
        };

        with_parsed_program(content, "unknown_variable", |program, content| {
            let mut ctx = DiagnosticCtx {
                backend: self,
                uri,
                content,
                diagnostics: Vec::new(),
            };

            for stmt in program.statements.iter() {
                collect_from_statement(stmt, &mut ctx, Some(&resolver));
            }

            out.extend(ctx.diagnostics);
        });
    }

    /// Look up which parameter positions are by-reference for a given call.
    ///
    /// Returns `Some(vec![...])` with 0-based argument positions that are
    /// by-reference, or `None` if the callee cannot be resolved.
    fn resolve_by_ref_positions(
        &self,
        call_kind: &ByRefCallKind<'_>,
        file_use_map: &std::collections::HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<Vec<usize>> {
        match call_kind {
            ByRefCallKind::Function(name) => {
                // Build FQN candidates: the raw name, plus the
                // namespace-qualified name if the file has a namespace.
                let fqn = crate::util::resolve_to_fqn(name, file_use_map, file_namespace);
                let mut candidates: Vec<&str> = vec![*name];
                if fqn != *name {
                    candidates.push(&fqn);
                }
                let func_info = self.find_or_load_function(&candidates)?;
                let positions: Vec<usize> = func_info
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
            ByRefCallKind::StaticMethod(class_name, method_name) => {
                let fqn = crate::util::resolve_to_fqn(class_name, file_use_map, file_namespace);
                let cls = self
                    .find_or_load_class(&fqn)
                    .or_else(|| self.find_or_load_class(class_name))?;
                let merged = crate::inheritance::resolve_class_with_inheritance(&cls, &|name| {
                    self.find_or_load_class(name)
                });
                let method = merged.get_method(method_name)?;
                let positions: Vec<usize> = method
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
            ByRefCallKind::Constructor(class_name) => {
                let fqn = crate::util::resolve_to_fqn(class_name, file_use_map, file_namespace);
                let cls = self
                    .find_or_load_class(&fqn)
                    .or_else(|| self.find_or_load_class(class_name))?;
                let merged = crate::inheritance::resolve_class_with_inheritance(&cls, &|name| {
                    self.find_or_load_class(name)
                });
                let ctor = merged.get_method("__construct")?;
                let positions: Vec<usize> = ctor
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
            ByRefCallKind::InstanceMethod(class_name, method_name) => {
                let fqn = crate::util::resolve_to_fqn(class_name, file_use_map, file_namespace);
                let cls = self
                    .find_or_load_class(&fqn)
                    .or_else(|| self.find_or_load_class(class_name))?;
                let merged = crate::inheritance::resolve_class_with_inheritance(&cls, &|name| {
                    self.find_or_load_class(name)
                });
                let method = merged.get_method(method_name)?;
                let positions: Vec<usize> = method
                    .parameters
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.is_reference)
                    .map(|(i, _)| i)
                    .collect();
                Some(positions)
            }
        }
    }
}

// ─── Internal context ───────────────────────────────────────────────────────

/// Collects diagnostics while walking the AST.
struct DiagnosticCtx<'a> {
    backend: &'a Backend,
    uri: &'a str,
    content: &'a str,
    diagnostics: Vec<Diagnostic>,
}

// ─── AST walking — find all function/method/closure bodies ──────────────────

/// Walk a top-level statement, recursing into namespace blocks,
/// class declarations, and function bodies.
fn collect_from_statement(
    stmt: &Statement<'_>,
    ctx: &mut DiagnosticCtx<'_>,
    resolver: Option<ByRefResolver<'_>>,
) {
    match stmt {
        Statement::Function(func) => {
            let body_start = func.body.left_brace.start.offset;
            let body_end = func.body.right_brace.end.offset;
            let scope = collect_function_scope_with_resolver(
                &func.parameter_list,
                func.body.statements.as_slice(),
                body_start,
                body_end,
                resolver,
            );
            check_scope(
                &scope,
                func.body.statements.as_slice(),
                ctx,
                false, // not a method
            );
        }
        Statement::Class(class) => {
            let class_name = crate::atom::bytes_to_str(class.name.value).to_string();
            collect_from_class_members(class.members.as_slice(), ctx, resolver, Some(&class_name));
        }
        Statement::Trait(tr) => {
            let trait_name = crate::atom::bytes_to_str(tr.name.value).to_string();
            collect_from_class_members(tr.members.as_slice(), ctx, resolver, Some(&trait_name));
        }
        Statement::Enum(en) => {
            let enum_name = crate::atom::bytes_to_str(en.name.value).to_string();
            collect_from_class_members(en.members.as_slice(), ctx, resolver, Some(&enum_name));
        }
        Statement::Interface(_) => {
            // Interfaces don't have method bodies.
        }
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                collect_from_statement(inner, ctx, resolver);
            }
        }
        // Top-level code (outside any function/class).
        _ => {
            // We don't diagnose top-level code because PHP's global
            // scope has too many implicit variable definitions
            // (include/require, extract in bootstrap files, etc.).
        }
    }
}

/// Walk class-like members to find method bodies.
fn collect_from_class_members(
    members: &[ClassLikeMember<'_>],
    ctx: &mut DiagnosticCtx<'_>,
    resolver: Option<ByRefResolver<'_>>,
    class_name: Option<&str>,
) {
    for member in members.iter() {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            let body_start = block.left_brace.start.offset;
            let body_end = block.right_brace.end.offset;

            let is_static = method
                .modifiers
                .iter()
                .any(|m| matches!(m, Modifier::Static(_)));

            let scope = collect_function_scope_with_kind_and_resolver(
                &method.parameter_list,
                block.statements.as_slice(),
                body_start,
                body_end,
                FrameKind::Method,
                resolver,
                class_name.map(|s| s.to_string()),
            );

            check_scope(&scope, block.statements.as_slice(), ctx, !is_static);
        }
    }
}

// ─── Scope analysis ─────────────────────────────────────────────────────────

/// Check a single scope (function/method body) for undefined variable reads.
///
/// For each variable read, we check whether the variable has been
/// written (assigned, declared as a parameter, etc.) at a **lower byte
/// offset** in the same frame.  Writes inside control-flow branches
/// (if/else, switch, try/catch) still count — we are conservative
/// about branches but strict about source order.  This catches the
/// common "used before assigned" typo while still avoiding false
/// positives from branch-dependent definitions.
fn check_scope(
    scope: &ScopeMap,
    statements: &[Statement<'_>],
    ctx: &mut DiagnosticCtx<'_>,
    this_is_defined: bool,
) {
    // Bail out early if the function uses features that make static
    // analysis unsound.
    if has_dynamic_variables(statements) || has_extract_call(statements) {
        return;
    }

    // Collect variable names referenced by compact() calls — these
    // variables are used by string name and should be treated as
    // defined.
    let compact_vars = collect_compact_vars(statements);

    // Collect variable names annotated with `/** @var Type $var */`
    // inline docblocks, each with the byte offset of its `$` sigil so it
    // can be treated as a scoped, source-ordered write rather than a
    // file-wide "always defined" name.
    let var_annotated = collect_var_annotations(ctx.content);

    // Collect byte offsets suppressed by the `@` error control
    // operator (e.g. `@$var`).
    let error_suppressed_offsets = collect_error_suppressed_offsets(statements);

    // Collect byte offsets of variables inside `isset()` and `empty()`.
    let guarded_offsets = collect_guarded_offsets(statements);

    // Bail out if there are no frames at all.
    if scope.frames.is_empty() {
        return;
    }

    // Build a set of "always-defined" names that do not require a
    // prior write: superglobals, compact-referenced vars, @var
    // annotations, and optionally $this.
    let mut always_defined: HashSet<&str> = HashSet::new();
    for sg in SUPERGLOBALS {
        always_defined.insert(sg);
    }
    if this_is_defined {
        always_defined.insert("$this");
    }
    for cv in &compact_vars {
        always_defined.insert(cv.as_str());
    }

    // Pre-compute the "own writes" for each frame: writes that are
    // directly inside the frame (not inside a nested sub-frame).
    let frame_own_writes: Vec<Vec<(&str, u32)>> = scope
        .frames
        .iter()
        .map(|frame| {
            let mut writes: Vec<(&str, u32)> = Vec::new();
            // Parameters (offset 0 = always before any read).
            for param in &frame.parameters {
                writes.push((param.as_str(), 0));
            }
            // Writes inside the frame body (excluding nested frames).
            for access in &scope.accesses {
                if !matches!(access.kind, AccessKind::Write | AccessKind::ReadWrite) {
                    continue;
                }
                if access.offset >= frame.start
                    && access.offset <= frame.end
                    && !is_in_nested_frame(access.offset, frame, &scope.frames)
                {
                    writes.push((access.name.as_str(), access.offset));
                }
            }
            // `/** @var Type $var */` annotations act as a write at the
            // annotation's offset, but only within the frame they appear in.
            for (name, offset) in &var_annotated {
                if *offset >= frame.start
                    && *offset <= frame.end
                    && !is_in_nested_frame(*offset, frame, &scope.frames)
                {
                    writes.push((name.as_str(), *offset));
                }
            }
            writes
        })
        .collect();

    // Process each frame independently.
    for (frame_idx, frame) in scope.frames.iter().enumerate() {
        // Build the list of writes visible to this frame by walking
        // up the parent-frame chain.  This correctly handles
        // arbitrary nesting depths (e.g. arrow fn inside closure
        // inside method, catch block inside closure, etc.).
        //
        // Visibility rules per frame kind:
        // - **Outermost / TopLevel / Function / Method**: own writes only
        // - **ArrowFunction / Catch**: parent's visible writes + own writes
        // - **Closure**: own writes only (captures are already recorded
        //   as Write accesses at body_start by the scope collector)
        let visible_writes = build_visible_writes(frame_idx, &scope.frames, &frame_own_writes);

        // Check reads: for each read, verify that a write of the same
        // name exists at a lower offset (or the name is always-defined).
        let frame_writes = &visible_writes;
        for access in &scope.accesses {
            if access.offset < frame.start || access.offset > frame.end {
                continue;
            }

            // Skip accesses inside nested frames.
            if is_in_nested_frame(access.offset, frame, &scope.frames) {
                continue;
            }

            if !matches!(access.kind, AccessKind::Read) {
                continue;
            }

            // Skip pseudo-variables.
            if access.name == "self" || access.name == "static" || access.name == "parent" {
                continue;
            }

            // Skip $this — even if not "defined", we don't flag it
            // (static methods will have $this reads flagged by other tools).
            if access.name == "$this" {
                continue;
            }

            // Skip if this read is guarded by isset() or empty().
            if guarded_offsets.contains(&access.offset) {
                continue;
            }

            // Skip if this read is under the @ error suppression operator.
            if error_suppressed_offsets.contains(&access.offset) {
                continue;
            }

            // Skip always-defined names.
            if always_defined.contains(access.name.as_str()) {
                continue;
            }

            // Check if any write of this variable exists at a lower
            // offset.  Parameters use offset 0 so they always qualify.
            let has_prior_write = frame_writes
                .iter()
                .any(|(name, off)| *name == access.name && *off < access.offset);

            if has_prior_write {
                continue;
            }

            // Emit diagnostic.
            let var_len = access.name.len();
            let range = match ctx.backend.offset_range_to_lsp_range(
                ctx.uri,
                ctx.content,
                access.offset as usize,
                access.offset as usize + var_len,
            ) {
                Some(r) => r,
                None => continue,
            };

            let message = format!("Undefined variable '{}'", access.name);

            ctx.diagnostics.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                UNKNOWN_VARIABLE_CODE,
                message,
            ));
        }
    }
}

/// Check whether a variable access at `offset` is inside a nested
/// frame (closure, arrow function) relative to the given `frame`.
/// Catch blocks are not treated as nested frames for this purpose
/// because variables defined in catch blocks leak into the enclosing
/// scope.
fn is_in_nested_frame(
    offset: u32,
    frame: &crate::scope_collector::Frame,
    frames: &[crate::scope_collector::Frame],
) -> bool {
    frames.iter().any(|f| {
        f.start > frame.start
            && f.end < frame.end
            && offset >= f.start
            && offset <= f.end
            && f.kind != FrameKind::Catch
    })
}

/// Find the index of the parent frame for `frame_idx`.
///
/// The parent is the smallest frame that strictly contains the given
/// frame.  Returns `None` for the outermost frame.
fn find_parent_frame_idx(
    frame_idx: usize,
    frames: &[crate::scope_collector::Frame],
) -> Option<usize> {
    let frame = &frames[frame_idx];
    let mut best: Option<usize> = None;
    for (i, candidate) in frames.iter().enumerate() {
        if i == frame_idx {
            continue;
        }
        // candidate must strictly contain frame
        if candidate.start <= frame.start
            && candidate.end >= frame.end
            && !(candidate.start == frame.start && candidate.end == frame.end)
        {
            match best {
                None => best = Some(i),
                Some(prev) => {
                    let prev_frame = &frames[prev];
                    // Pick the tighter (smaller) enclosing frame.
                    if (candidate.end - candidate.start) < (prev_frame.end - prev_frame.start) {
                        best = Some(i);
                    }
                }
            }
        }
    }
    best
}

/// Build the set of writes visible to the frame at `frame_idx` by
/// walking up the parent chain.
///
/// - **Arrow functions and catch blocks** inherit all writes visible
///   to their parent, plus their own direct writes.
/// - **Closures** see only their own direct writes (captures are
///   already recorded as Write accesses by the scope collector).
/// - **Outermost / function / method** frames see only their own
///   direct writes.
fn build_visible_writes<'a>(
    frame_idx: usize,
    frames: &[crate::scope_collector::Frame],
    frame_own_writes: &[Vec<(&'a str, u32)>],
) -> Vec<(&'a str, u32)> {
    let frame = &frames[frame_idx];
    let own = &frame_own_writes[frame_idx];

    match frame.kind {
        FrameKind::ArrowFunction | FrameKind::Catch => {
            // Inherit parent's visible writes, then add our own.
            let parent_writes = match find_parent_frame_idx(frame_idx, frames) {
                Some(parent_idx) => build_visible_writes(parent_idx, frames, frame_own_writes),
                None => Vec::new(),
            };
            let mut combined = parent_writes;
            combined.extend_from_slice(own);
            combined
        }
        _ => {
            // Closures, outermost, functions, methods: own writes only.
            own.clone()
        }
    }
}

// ─── Dynamic variable / extract detection ───────────────────────────────────

/// Returns `true` if the statements contain variable variables (`$$x`)
/// anywhere in the function body.
fn has_dynamic_variables(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_dynamic_var(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_dynamic_var(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_dynamic_var(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_dynamic_var(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_dynamic_var(v)),
        Statement::If(if_stmt) => {
            if expr_has_dynamic_var(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_dynamic_var(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_dynamic_var(clause.condition)
                            || stmt_has_dynamic_var(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_dynamic_var(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_dynamic_var(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_dynamic_var(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_dynamic_var(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_dynamic_var(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_dynamic_var(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_dynamic_var(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_dynamic_var(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_dynamic_var(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::DoWhile(dw) => {
            stmt_has_dynamic_var(dw.statement) || expr_has_dynamic_var(dw.condition)
        }
        Statement::For(for_stmt) => {
            for_stmt
                .initializations
                .iter()
                .any(|e| expr_has_dynamic_var(e))
                || for_stmt.conditions.iter().any(|e| expr_has_dynamic_var(e))
                || for_stmt.increments.iter().any(|e| expr_has_dynamic_var(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_dynamic_var(s),
                    ForBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                }
        }
        Statement::Switch(sw) => {
            expr_has_dynamic_var(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_dynamic_var(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                    SwitchCase::Default(dc) => {
                        dc.statements.iter().any(|s| stmt_has_dynamic_var(s))
                    }
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_dynamic_var(s))
                || try_stmt
                    .catch_clauses
                    .iter()
                    .any(|c| c.block.statements.iter().any(|s| stmt_has_dynamic_var(s)))
                || try_stmt
                    .finally_clause
                    .as_ref()
                    .is_some_and(|f| f.block.statements.iter().any(|s| stmt_has_dynamic_var(s)))
        }
        Statement::Block(block) => block.statements.iter().any(|s| stmt_has_dynamic_var(s)),
        Statement::Unset(_) => false,
        Statement::Global(_) => false,
        Statement::Static(_) => false,
        _ => false,
    }
}

fn expr_has_dynamic_var(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Variable(Variable::Indirect(_)) => true,
        Expression::Variable(Variable::Nested(_)) => true,
        Expression::Variable(_) => false,
        Expression::Assignment(a) => expr_has_dynamic_var(a.lhs) || expr_has_dynamic_var(a.rhs),
        Expression::Binary(b) => expr_has_dynamic_var(b.lhs) || expr_has_dynamic_var(b.rhs),
        Expression::UnaryPrefix(u) => expr_has_dynamic_var(u.operand),
        Expression::UnaryPostfix(u) => expr_has_dynamic_var(u.operand),
        Expression::Parenthesized(p) => expr_has_dynamic_var(p.expression),
        Expression::Call(call) => match call {
            Call::Function(fc) => {
                expr_has_dynamic_var(fc.function)
                    || fc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::Method(mc) => {
                expr_has_dynamic_var(mc.object)
                    || mc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::NullSafeMethod(mc) => {
                expr_has_dynamic_var(mc.object)
                    || mc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
            Call::StaticMethod(sc) => {
                expr_has_dynamic_var(sc.class)
                    || sc
                        .argument_list
                        .arguments
                        .iter()
                        .any(|a| expr_has_dynamic_var(a.value()))
            }
        },
        Expression::Access(access) => match access {
            Access::Property(pa) => expr_has_dynamic_var(pa.object),
            Access::NullSafeProperty(pa) => expr_has_dynamic_var(pa.object),
            Access::StaticProperty(spa) => expr_has_dynamic_var(spa.class),
            Access::ClassConstant(cca) => expr_has_dynamic_var(cca.class),
        },
        Expression::ArrayAccess(aa) => {
            expr_has_dynamic_var(aa.array) || expr_has_dynamic_var(aa.index)
        }
        Expression::Conditional(c) => {
            expr_has_dynamic_var(c.condition)
                || c.then.is_some_and(|t| expr_has_dynamic_var(t))
                || expr_has_dynamic_var(c.r#else)
        }
        Expression::Instantiation(inst) => {
            expr_has_dynamic_var(inst.class)
                || inst
                    .argument_list
                    .as_ref()
                    .is_some_and(|al| al.arguments.iter().any(|a| expr_has_dynamic_var(a.value())))
        }
        Expression::Array(arr) => arr.elements.iter().any(|e| array_elem_has_dynamic_var(e)),
        Expression::LegacyArray(arr) => arr.elements.iter().any(|e| array_elem_has_dynamic_var(e)),
        Expression::Throw(t) => expr_has_dynamic_var(t.exception),
        Expression::Clone(c) => expr_has_dynamic_var(c.object),
        Expression::Match(m) => {
            expr_has_dynamic_var(m.expression)
                || m.arms.iter().any(|arm| match arm {
                    MatchArm::Expression(ea) => {
                        ea.conditions.iter().any(|c| expr_has_dynamic_var(c))
                            || expr_has_dynamic_var(ea.expression)
                    }
                    MatchArm::Default(da) => expr_has_dynamic_var(da.expression),
                })
        }
        // Closures and arrow functions have their own scope — don't
        // recurse into them for dynamic variable detection.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

fn array_elem_has_dynamic_var(elem: &ArrayElement<'_>) -> bool {
    match elem {
        ArrayElement::KeyValue(kv) => {
            expr_has_dynamic_var(kv.key) || expr_has_dynamic_var(kv.value)
        }
        ArrayElement::Value(v) => expr_has_dynamic_var(v.value),
        ArrayElement::Variadic(s) => expr_has_dynamic_var(s.value),
        ArrayElement::Missing(_) => false,
    }
}

/// Returns `true` if the statements contain a call to `extract()`.
fn has_extract_call(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_extract(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_extract(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_extract(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_extract(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_extract(v)),
        Statement::If(if_stmt) => {
            if expr_has_extract(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_extract(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_extract(clause.condition) || stmt_has_extract(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_extract(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_extract(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_extract(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_extract(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_extract(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_extract(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_extract(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_extract(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_extract(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_extract(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_extract(s))
                    }
                }
        }
        Statement::DoWhile(dw) => stmt_has_extract(dw.statement) || expr_has_extract(dw.condition),
        Statement::For(for_stmt) => {
            for_stmt.initializations.iter().any(|e| expr_has_extract(e))
                || for_stmt.conditions.iter().any(|e| expr_has_extract(e))
                || for_stmt.increments.iter().any(|e| expr_has_extract(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_extract(s),
                    ForBody::ColonDelimited(b) => b.statements.iter().any(|s| stmt_has_extract(s)),
                }
        }
        Statement::Switch(sw) => {
            expr_has_extract(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_extract(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_extract(s))
                    }
                    SwitchCase::Default(dc) => dc.statements.iter().any(|s| stmt_has_extract(s)),
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_extract(s))
                || try_stmt
                    .catch_clauses
                    .iter()
                    .any(|c| c.block.statements.iter().any(|s| stmt_has_extract(s)))
                || try_stmt
                    .finally_clause
                    .as_ref()
                    .is_some_and(|f| f.block.statements.iter().any(|s| stmt_has_extract(s)))
        }
        Statement::Block(block) => block.statements.iter().any(|s| stmt_has_extract(s)),
        _ => false,
    }
}

fn expr_has_extract(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"extract")
            {
                return true;
            }
            // Check arguments recursively.
            fc.argument_list
                .arguments
                .iter()
                .any(|a| expr_has_extract(a.value()))
        }
        Expression::Assignment(a) => expr_has_extract(a.lhs) || expr_has_extract(a.rhs),
        Expression::Binary(b) => expr_has_extract(b.lhs) || expr_has_extract(b.rhs),
        Expression::UnaryPrefix(u) => expr_has_extract(u.operand),
        Expression::UnaryPostfix(u) => expr_has_extract(u.operand),
        Expression::Parenthesized(p) => expr_has_extract(p.expression),
        Expression::Conditional(c) => {
            expr_has_extract(c.condition)
                || c.then.is_some_and(|t| expr_has_extract(t))
                || expr_has_extract(c.r#else)
        }
        Expression::Call(Call::Method(mc)) => {
            expr_has_extract(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            expr_has_extract(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            expr_has_extract(sc.class)
                || sc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_extract(a.value()))
        }
        // Don't recurse into closures/arrow functions.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

// ─── compact() variable collection ──────────────────────────────────────────

/// Collect variable names referenced by `compact('var1', 'var2', …)`
/// calls.  These variables are used by string name and should be
/// considered defined.
pub(crate) fn collect_compact_vars(statements: &[Statement<'_>]) -> HashSet<String> {
    let mut vars = HashSet::new();
    for stmt in statements {
        collect_compact_from_stmt(stmt, &mut vars);
    }
    vars
}

fn collect_compact_from_stmt(stmt: &Statement<'_>, vars: &mut HashSet<String>) {
    match stmt {
        Statement::Expression(es) => collect_compact_from_expr(es.expression, vars),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_compact_from_expr(v, vars);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_compact_from_expr(v, vars);
            }
        }
        Statement::If(if_stmt) => {
            collect_compact_from_expr(if_stmt.condition, vars);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_compact_from_stmt(body.statement, vars);
                    for clause in body.else_if_clauses.iter() {
                        collect_compact_from_expr(clause.condition, vars);
                        collect_compact_from_stmt(clause.statement, vars);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_compact_from_stmt(el.statement, vars);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_compact_from_expr(clause.condition, vars);
                        for s in clause.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_compact_from_expr(foreach.expression, vars);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_compact_from_stmt(s, vars),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_compact_from_expr(w.condition, vars);
            match &w.body {
                WhileBody::Statement(s) => collect_compact_from_stmt(s, vars),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_compact_from_stmt(dw.statement, vars);
            collect_compact_from_expr(dw.condition, vars);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_compact_from_expr(e, vars);
            }
            for e in for_stmt.conditions.iter() {
                collect_compact_from_expr(e, vars);
            }
            for e in for_stmt.increments.iter() {
                collect_compact_from_expr(e, vars);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_compact_from_stmt(s, vars),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_compact_from_stmt(s, vars);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_compact_from_expr(sw.expression, vars);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_compact_from_expr(sc.expression, vars);
                        for s in sc.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_compact_from_stmt(s, vars);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_compact_from_stmt(s, vars);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_compact_from_stmt(s, vars);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_compact_from_stmt(s, vars);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_compact_from_stmt(s, vars);
            }
        }
        _ => {}
    }
}

fn collect_compact_from_expr(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"compact")
            {
                // Each argument is a variable name (string literal) or
                // an array of names (possibly nested), matching the
                // forms compact() accepts.
                for arg in fc.argument_list.arguments.iter() {
                    collect_compact_name_from_arg(arg.value(), vars);
                }
            }
            // Also recurse into arguments for nested compact() calls.
            for arg in fc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Assignment(a) => {
            collect_compact_from_expr(a.lhs, vars);
            collect_compact_from_expr(a.rhs, vars);
        }
        Expression::Binary(b) => {
            collect_compact_from_expr(b.lhs, vars);
            collect_compact_from_expr(b.rhs, vars);
        }
        Expression::Parenthesized(p) => collect_compact_from_expr(p.expression, vars),
        Expression::Conditional(c) => {
            collect_compact_from_expr(c.condition, vars);
            if let Some(t) = c.then {
                collect_compact_from_expr(t, vars);
            }
            collect_compact_from_expr(c.r#else, vars);
        }
        Expression::Call(Call::Method(mc)) => {
            collect_compact_from_expr(mc.object, vars);
            for arg in mc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            collect_compact_from_expr(mc.object, vars);
            for arg in mc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            collect_compact_from_expr(sc.class, vars);
            for arg in sc.argument_list.arguments.iter() {
                collect_compact_from_expr(arg.value(), vars);
            }
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

/// Collect variable names from a single `compact()` argument. A string
/// literal names a variable directly; an array literal is descended
/// into recursively so `compact(['a', ['b']])` collects both names.
fn collect_compact_name_from_arg(expr: &Expression<'_>, vars: &mut HashSet<String>) {
    match expr {
        Expression::Literal(Literal::String(s)) => {
            // `value` is the interpreted string content (without
            // quotes); fall back to `raw` and strip quotes manually
            // if `value` is `None`.
            let name: &str = if let Some(v) = s.value {
                crate::atom::bytes_to_str(v)
            } else {
                let raw = crate::atom::bytes_to_str(s.raw);
                raw.strip_prefix('\'')
                    .or_else(|| raw.strip_prefix('"'))
                    .and_then(|inner| inner.strip_suffix('\'').or_else(|| inner.strip_suffix('"')))
                    .unwrap_or(raw)
            };
            if !name.is_empty() {
                vars.insert(format!("${}", name));
            }
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                collect_compact_name_from_elem(elem, vars);
            }
        }
        _ => {}
    }
}

/// Collect variable names from one element of an array passed to
/// `compact()`. Keys are ignored; values are names or nested arrays.
fn collect_compact_name_from_elem(elem: &ArrayElement<'_>, vars: &mut HashSet<String>) {
    match elem {
        ArrayElement::KeyValue(kv) => collect_compact_name_from_arg(kv.value, vars),
        ArrayElement::Value(v) => collect_compact_name_from_arg(v.value, vars),
        ArrayElement::Variadic(s) => collect_compact_name_from_arg(s.value, vars),
        ArrayElement::Missing(_) => {}
    }
}

// ─── get_defined_vars() detection ───────────────────────────────────────────

/// Returns true if the statements contain a call to `get_defined_vars()`.
/// When present in a scope, all variables defined in that scope are
/// considered used (e.g. for debug dumps), so unused-variable diagnostics
/// should be suppressed for them.
pub(crate) fn has_get_defined_vars(statements: &[Statement<'_>]) -> bool {
    for stmt in statements {
        if stmt_has_get_defined_vars(stmt) {
            return true;
        }
    }
    false
}

fn stmt_has_get_defined_vars(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::Expression(es) => expr_has_get_defined_vars(es.expression),
        Statement::Return(ret) => ret.value.is_some_and(|v| expr_has_get_defined_vars(v)),
        Statement::Echo(echo) => echo.values.iter().any(|v| expr_has_get_defined_vars(v)),
        Statement::If(if_stmt) => {
            if expr_has_get_defined_vars(if_stmt.condition) {
                return true;
            }
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    if stmt_has_get_defined_vars(body.statement) {
                        return true;
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_get_defined_vars(clause.condition)
                            || stmt_has_get_defined_vars(clause.statement)
                        {
                            return true;
                        }
                    }
                    if let Some(ref el) = body.else_clause
                        && stmt_has_get_defined_vars(el.statement)
                    {
                        return true;
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        if stmt_has_get_defined_vars(s) {
                            return true;
                        }
                    }
                    for clause in body.else_if_clauses.iter() {
                        if expr_has_get_defined_vars(clause.condition) {
                            return true;
                        }
                        for s in clause.statements.iter() {
                            if stmt_has_get_defined_vars(s) {
                                return true;
                            }
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            if stmt_has_get_defined_vars(s) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        }
        Statement::Foreach(foreach) => {
            expr_has_get_defined_vars(foreach.expression)
                || match &foreach.body {
                    ForeachBody::Statement(s) => stmt_has_get_defined_vars(s),
                    ForeachBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::While(w) => {
            expr_has_get_defined_vars(w.condition)
                || match &w.body {
                    WhileBody::Statement(s) => stmt_has_get_defined_vars(s),
                    WhileBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::DoWhile(dw) => {
            stmt_has_get_defined_vars(dw.statement) || expr_has_get_defined_vars(dw.condition)
        }
        Statement::For(for_stmt) => {
            for_stmt
                .initializations
                .iter()
                .any(|e| expr_has_get_defined_vars(e))
                || for_stmt
                    .conditions
                    .iter()
                    .any(|e| expr_has_get_defined_vars(e))
                || for_stmt
                    .increments
                    .iter()
                    .any(|e| expr_has_get_defined_vars(e))
                || match &for_stmt.body {
                    ForBody::Statement(s) => stmt_has_get_defined_vars(s),
                    ForBody::ColonDelimited(b) => {
                        b.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                }
        }
        Statement::Switch(sw) => {
            expr_has_get_defined_vars(sw.expression)
                || sw.body.cases().iter().any(|c| match c {
                    SwitchCase::Expression(sc) => {
                        expr_has_get_defined_vars(sc.expression)
                            || sc.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                    SwitchCase::Default(dc) => {
                        dc.statements.iter().any(|s| stmt_has_get_defined_vars(s))
                    }
                })
        }
        Statement::Try(try_stmt) => {
            try_stmt
                .block
                .statements
                .iter()
                .any(|s| stmt_has_get_defined_vars(s))
                || try_stmt.catch_clauses.iter().any(|c| {
                    c.block
                        .statements
                        .iter()
                        .any(|s| stmt_has_get_defined_vars(s))
                })
                || try_stmt.finally_clause.as_ref().is_some_and(|f| {
                    f.block
                        .statements
                        .iter()
                        .any(|s| stmt_has_get_defined_vars(s))
                })
        }
        Statement::Block(block) => block
            .statements
            .iter()
            .any(|s| stmt_has_get_defined_vars(s)),
        _ => false,
    }
}

fn expr_has_get_defined_vars(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Call(Call::Function(fc)) => {
            if let Expression::Identifier(ident) = fc.function
                && ident.value().eq_ignore_ascii_case(b"get_defined_vars")
            {
                return true;
            }
            expr_has_get_defined_vars(fc.function)
                // Recurse into arguments for nested calls.
                || fc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::Method(mc)) => {
            expr_has_get_defined_vars(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            expr_has_get_defined_vars(mc.object)
                || mc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            expr_has_get_defined_vars(sc.class)
                || sc
                    .argument_list
                    .arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
        }
        Expression::Access(access) => match access {
            Access::Property(pa) => expr_has_get_defined_vars(pa.object),
            Access::NullSafeProperty(pa) => expr_has_get_defined_vars(pa.object),
            Access::StaticProperty(spa) => expr_has_get_defined_vars(spa.class),
            Access::ClassConstant(cca) => expr_has_get_defined_vars(cca.class),
        },
        Expression::ArrayAccess(aa) => {
            expr_has_get_defined_vars(aa.array) || expr_has_get_defined_vars(aa.index)
        }
        Expression::ArrayAppend(append) => expr_has_get_defined_vars(append.array),
        Expression::Array(arr) => arr
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::LegacyArray(arr) => arr
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::List(list) => list
            .elements
            .iter()
            .any(|e| array_elem_has_get_defined_vars(e)),
        Expression::Assignment(a) => {
            expr_has_get_defined_vars(a.lhs) || expr_has_get_defined_vars(a.rhs)
        }
        Expression::Binary(b) => {
            expr_has_get_defined_vars(b.lhs) || expr_has_get_defined_vars(b.rhs)
        }
        Expression::UnaryPrefix(u) => expr_has_get_defined_vars(u.operand),
        Expression::UnaryPostfix(u) => expr_has_get_defined_vars(u.operand),
        Expression::Parenthesized(p) => expr_has_get_defined_vars(p.expression),
        Expression::Conditional(c) => {
            expr_has_get_defined_vars(c.condition)
                || c.then.is_some_and(|t| expr_has_get_defined_vars(t))
                || expr_has_get_defined_vars(c.r#else)
        }
        Expression::Instantiation(inst) => {
            expr_has_get_defined_vars(inst.class)
                || inst.argument_list.as_ref().is_some_and(|al| {
                    al.arguments
                        .iter()
                        .any(|a| expr_has_get_defined_vars(a.value()))
                })
        }
        Expression::Throw(t) => expr_has_get_defined_vars(t.exception),
        Expression::Clone(c) => expr_has_get_defined_vars(c.object),
        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => yv.value.is_some_and(expr_has_get_defined_vars),
            Yield::Pair(yp) => {
                expr_has_get_defined_vars(yp.key) || expr_has_get_defined_vars(yp.value)
            }
            Yield::From(yf) => expr_has_get_defined_vars(yf.iterator),
        },
        Expression::Match(m) => {
            expr_has_get_defined_vars(m.expression)
                || m.arms.iter().any(|arm| match arm {
                    MatchArm::Expression(ea) => {
                        ea.conditions.iter().any(|c| expr_has_get_defined_vars(c))
                            || expr_has_get_defined_vars(ea.expression)
                    }
                    MatchArm::Default(da) => expr_has_get_defined_vars(da.expression),
                })
        }
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => isset.values.iter().any(|v| expr_has_get_defined_vars(v)),
            Construct::Empty(empty) => expr_has_get_defined_vars(empty.value),
            Construct::Eval(eval) => expr_has_get_defined_vars(eval.value),
            Construct::Include(inc) => expr_has_get_defined_vars(inc.value),
            Construct::IncludeOnce(inc) => expr_has_get_defined_vars(inc.value),
            Construct::Require(req) => expr_has_get_defined_vars(req.value),
            Construct::RequireOnce(req) => expr_has_get_defined_vars(req.value),
            Construct::Print(print) => expr_has_get_defined_vars(print.value),
            Construct::Exit(exit) => exit.arguments.as_ref().is_some_and(|args| {
                args.arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
            }),
            Construct::Die(die) => die.arguments.as_ref().is_some_and(|args| {
                args.arguments
                    .iter()
                    .any(|a| expr_has_get_defined_vars(a.value()))
            }),
        },
        Expression::CompositeString(composite) => composite.parts().iter().any(|part| match part {
            StringPart::Expression(inner_expr) => expr_has_get_defined_vars(inner_expr),
            StringPart::BracedExpression(braced) => expr_has_get_defined_vars(braced.expression),
            StringPart::Literal(_) => false,
        }),
        Expression::Pipe(pipe) => {
            expr_has_get_defined_vars(pipe.input) || expr_has_get_defined_vars(pipe.callable)
        }
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(func_pa) => expr_has_get_defined_vars(func_pa.function),
            PartialApplication::Method(method_pa) => expr_has_get_defined_vars(method_pa.object),
            PartialApplication::StaticMethod(static_pa) => {
                expr_has_get_defined_vars(static_pa.class)
            }
        },
        Expression::AnonymousClass(anon) => anon.argument_list.as_ref().is_some_and(|args| {
            args.arguments
                .iter()
                .any(|a| a.value().is_some_and(expr_has_get_defined_vars))
        }),
        // Don't recurse into closures/arrow functions.
        Expression::Closure(_) | Expression::ArrowFunction(_) => false,
        _ => false,
    }
}

fn array_elem_has_get_defined_vars(elem: &ArrayElement<'_>) -> bool {
    match elem {
        ArrayElement::KeyValue(kv) => {
            expr_has_get_defined_vars(kv.key) || expr_has_get_defined_vars(kv.value)
        }
        ArrayElement::Value(v) => expr_has_get_defined_vars(v.value),
        ArrayElement::Variadic(s) => expr_has_get_defined_vars(s.value),
        ArrayElement::Missing(_) => false,
    }
}

// ─── @var annotation collection ─────────────────────────────────────────────

/// Scan the source text for `/** @var Type $varName */` inline
/// docblocks and return each declared variable name paired with the byte
/// offset of its `$` sigil.
///
/// The offset lets callers treat the annotation as a write at that
/// position so it (a) only defines the variable within the scope it
/// appears in, and (b) follows the same "prior write in source order"
/// rule as ordinary assignments.
fn collect_var_annotations(content: &str) -> Vec<(String, u32)> {
    let mut vars = Vec::new();
    // Look for patterns like: @var SomeType $varName
    // The regex-like scan: find `@var ` followed by a type, then `$name`.
    let mut line_start = 0usize;
    for line in content.lines() {
        // `lines()` strips the line terminator; track the running byte
        // offset so we can report absolute positions.
        let this_line_start = line_start;
        line_start += line.len() + 1; // +1 for the stripped '\n'

        if !line.contains("@var") {
            continue;
        }
        // Find `@var` and extract the variable name after the type.
        if let Some(var_pos) = line.find("@var") {
            let after_var_off = var_pos + 4;
            let after_var = &line[after_var_off..];
            let ws = after_var.len() - after_var.trim_start().len();
            let after_var = after_var.trim_start();
            // Skip the type (everything before the $).
            if let Some(dollar_pos) = after_var.find('$') {
                let var_part = &after_var[dollar_pos..];
                // Extract the variable name: $[a-zA-Z_][a-zA-Z0-9_]*
                let name_end = var_part
                    .char_indices()
                    .skip(1) // skip the $
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map(|(i, _)| i)
                    .unwrap_or(var_part.len());
                let var_name = &var_part[..name_end];
                // Trim trailing `*/` if present.
                let var_name = var_name.trim_end_matches("*/").trim();
                if var_name.len() > 1 {
                    let dollar_offset = this_line_start + after_var_off + ws + dollar_pos;
                    vars.push((var_name.to_string(), dollar_offset as u32));
                }
            }
        }
    }
    vars
}

// ─── Error suppression (@) offset collection ────────────────────────────────

/// Collect byte offsets of variable reads that are directly under the
/// `@` error suppression operator (e.g. `@$var`).
fn collect_error_suppressed_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let mut offsets = HashSet::new();
    for stmt in statements {
        collect_suppressed_from_stmt(stmt, &mut offsets);
    }
    offsets
}

fn collect_suppressed_from_stmt(stmt: &Statement<'_>, offsets: &mut HashSet<u32>) {
    match stmt {
        Statement::Expression(es) => collect_suppressed_from_expr(es.expression, false, offsets),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_suppressed_from_expr(v, false, offsets);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_suppressed_from_expr(v, false, offsets);
            }
        }
        Statement::If(if_stmt) => {
            collect_suppressed_from_expr(if_stmt.condition, false, offsets);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_suppressed_from_stmt(body.statement, offsets);
                    for clause in body.else_if_clauses.iter() {
                        collect_suppressed_from_expr(clause.condition, false, offsets);
                        collect_suppressed_from_stmt(clause.statement, offsets);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_suppressed_from_stmt(el.statement, offsets);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_suppressed_from_expr(clause.condition, false, offsets);
                        for s in clause.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_suppressed_from_expr(foreach.expression, false, offsets);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_suppressed_from_expr(w.condition, false, offsets);
            match &w.body {
                WhileBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_suppressed_from_stmt(dw.statement, offsets);
            collect_suppressed_from_expr(dw.condition, false, offsets);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            for e in for_stmt.conditions.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            for e in for_stmt.increments.iter() {
                collect_suppressed_from_expr(e, false, offsets);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_suppressed_from_stmt(s, offsets),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_suppressed_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_suppressed_from_expr(sw.expression, false, offsets);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_suppressed_from_expr(sc.expression, false, offsets);
                        for s in sc.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_suppressed_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_suppressed_from_stmt(s, offsets);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_suppressed_from_stmt(s, offsets);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_suppressed_from_stmt(s, offsets);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_suppressed_from_stmt(s, offsets);
            }
        }
        _ => {}
    }
}

fn collect_suppressed_from_expr(
    expr: &Expression<'_>,
    under_error_control: bool,
    offsets: &mut HashSet<u32>,
) {
    match expr {
        Expression::UnaryPrefix(unary) if unary.operator.is_error_control() => {
            // The operand is under @.
            collect_suppressed_from_expr(unary.operand, true, offsets);
        }
        Expression::Variable(Variable::Direct(dv)) if under_error_control => {
            offsets.insert(dv.span().start.offset);
        }
        Expression::UnaryPrefix(unary) => {
            collect_suppressed_from_expr(unary.operand, under_error_control, offsets);
        }
        Expression::UnaryPostfix(unary) => {
            collect_suppressed_from_expr(unary.operand, under_error_control, offsets);
        }
        Expression::Assignment(a) => {
            collect_suppressed_from_expr(a.lhs, under_error_control, offsets);
            collect_suppressed_from_expr(a.rhs, under_error_control, offsets);
        }
        Expression::Binary(b) => {
            collect_suppressed_from_expr(b.lhs, under_error_control, offsets);
            collect_suppressed_from_expr(b.rhs, under_error_control, offsets);
        }
        Expression::Parenthesized(p) => {
            collect_suppressed_from_expr(p.expression, under_error_control, offsets);
        }
        Expression::Call(Call::Function(fc)) => {
            collect_suppressed_from_expr(fc.function, under_error_control, offsets);
            for arg in fc.argument_list.arguments.iter() {
                collect_suppressed_from_expr(arg.value(), under_error_control, offsets);
            }
        }
        Expression::Call(Call::Method(mc)) => {
            collect_suppressed_from_expr(mc.object, under_error_control, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_suppressed_from_expr(arg.value(), under_error_control, offsets);
            }
        }
        Expression::Access(Access::Property(pa)) => {
            collect_suppressed_from_expr(pa.object, under_error_control, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_suppressed_from_expr(pa.object, under_error_control, offsets);
        }
        Expression::ArrayAccess(aa) => {
            collect_suppressed_from_expr(aa.array, under_error_control, offsets);
            collect_suppressed_from_expr(aa.index, under_error_control, offsets);
        }
        Expression::Conditional(c) => {
            collect_suppressed_from_expr(c.condition, under_error_control, offsets);
            if let Some(t) = c.then {
                collect_suppressed_from_expr(t, under_error_control, offsets);
            }
            collect_suppressed_from_expr(c.r#else, under_error_control, offsets);
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

// ─── isset() / empty() guarded offset collection ───────────────────────────

/// Collect byte offsets of variable reads that appear directly inside
/// `isset()` or `empty()` calls.  These variables are being guarded,
/// not used.
fn collect_guarded_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let mut offsets = HashSet::new();
    for stmt in statements {
        collect_guarded_from_stmt(stmt, &mut offsets);
    }
    offsets
}

fn collect_guarded_from_stmt(stmt: &Statement<'_>, offsets: &mut HashSet<u32>) {
    match stmt {
        Statement::Expression(es) => collect_guarded_from_expr(es.expression, false, offsets),
        Statement::Return(ret) => {
            if let Some(v) = ret.value {
                collect_guarded_from_expr(v, false, offsets);
            }
        }
        Statement::Echo(echo) => {
            for v in echo.values.iter() {
                collect_guarded_from_expr(v, false, offsets);
            }
        }
        Statement::If(if_stmt) => {
            collect_guarded_from_expr(if_stmt.condition, false, offsets);
            match &if_stmt.body {
                IfBody::Statement(body) => {
                    collect_guarded_from_stmt(body.statement, offsets);
                    for clause in body.else_if_clauses.iter() {
                        collect_guarded_from_expr(clause.condition, false, offsets);
                        collect_guarded_from_stmt(clause.statement, offsets);
                    }
                    if let Some(ref el) = body.else_clause {
                        collect_guarded_from_stmt(el.statement, offsets);
                    }
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                    for clause in body.else_if_clauses.iter() {
                        collect_guarded_from_expr(clause.condition, false, offsets);
                        for s in clause.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                    if let Some(ref el) = body.else_clause {
                        for s in el.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            collect_guarded_from_expr(foreach.expression, false, offsets);
            match &foreach.body {
                ForeachBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                ForeachBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::While(w) => {
            collect_guarded_from_expr(w.condition, false, offsets);
            match &w.body {
                WhileBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                WhileBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            collect_guarded_from_stmt(dw.statement, offsets);
            collect_guarded_from_expr(dw.condition, false, offsets);
        }
        Statement::For(for_stmt) => {
            for e in for_stmt.initializations.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            for e in for_stmt.conditions.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            for e in for_stmt.increments.iter() {
                collect_guarded_from_expr(e, false, offsets);
            }
            match &for_stmt.body {
                ForBody::Statement(s) => collect_guarded_from_stmt(s, offsets),
                ForBody::ColonDelimited(b) => {
                    for s in b.statements.iter() {
                        collect_guarded_from_stmt(s, offsets);
                    }
                }
            }
        }
        Statement::Switch(sw) => {
            collect_guarded_from_expr(sw.expression, false, offsets);
            for case in sw.body.cases().iter() {
                match case {
                    SwitchCase::Expression(sc) => {
                        collect_guarded_from_expr(sc.expression, false, offsets);
                        for s in sc.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                    SwitchCase::Default(dc) => {
                        for s in dc.statements.iter() {
                            collect_guarded_from_stmt(s, offsets);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                collect_guarded_from_stmt(s, offsets);
            }
            for catch in try_stmt.catch_clauses.iter() {
                for s in catch.block.statements.iter() {
                    collect_guarded_from_stmt(s, offsets);
                }
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    collect_guarded_from_stmt(s, offsets);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                collect_guarded_from_stmt(s, offsets);
            }
        }
        _ => {}
    }
}

fn collect_guarded_from_expr(
    expr: &Expression<'_>,
    inside_guard: bool,
    offsets: &mut HashSet<u32>,
) {
    match expr {
        Expression::Construct(Construct::Isset(isset)) => {
            // All variables inside isset() are guarded.
            for val in isset.values.iter() {
                collect_guard_targets(val, offsets);
            }
        }
        Expression::Construct(Construct::Empty(empty)) => {
            // The variable inside empty() is guarded.
            collect_guard_targets(empty.value, offsets);
        }
        Expression::UnaryPrefix(unary) => {
            collect_guarded_from_expr(unary.operand, inside_guard, offsets);
        }
        Expression::UnaryPostfix(unary) => {
            collect_guarded_from_expr(unary.operand, inside_guard, offsets);
        }
        Expression::Assignment(a) => {
            collect_guarded_from_expr(a.lhs, inside_guard, offsets);
            collect_guarded_from_expr(a.rhs, inside_guard, offsets);
        }
        Expression::Binary(b) => {
            collect_guarded_from_expr(b.lhs, inside_guard, offsets);
            collect_guarded_from_expr(b.rhs, inside_guard, offsets);
        }
        Expression::Parenthesized(p) => {
            collect_guarded_from_expr(p.expression, inside_guard, offsets);
        }
        Expression::Conditional(c) => {
            collect_guarded_from_expr(c.condition, inside_guard, offsets);
            if let Some(t) = c.then {
                collect_guarded_from_expr(t, inside_guard, offsets);
            }
            collect_guarded_from_expr(c.r#else, inside_guard, offsets);
        }
        Expression::Call(Call::Function(fc)) => {
            collect_guarded_from_expr(fc.function, inside_guard, offsets);
            for arg in fc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::Method(mc)) => {
            collect_guarded_from_expr(mc.object, inside_guard, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::NullSafeMethod(mc)) => {
            collect_guarded_from_expr(mc.object, inside_guard, offsets);
            for arg in mc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            collect_guarded_from_expr(sc.class, inside_guard, offsets);
            for arg in sc.argument_list.arguments.iter() {
                collect_guarded_from_expr(arg.value(), inside_guard, offsets);
            }
        }
        Expression::Access(Access::Property(pa)) => {
            collect_guarded_from_expr(pa.object, inside_guard, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_guarded_from_expr(pa.object, inside_guard, offsets);
        }
        Expression::ArrayAccess(aa) => {
            collect_guarded_from_expr(aa.array, inside_guard, offsets);
            collect_guarded_from_expr(aa.index, inside_guard, offsets);
        }
        Expression::Array(arr) => {
            for e in arr.elements.iter() {
                collect_guarded_from_array_elem(e, inside_guard, offsets);
            }
        }
        Expression::LegacyArray(arr) => {
            for e in arr.elements.iter() {
                collect_guarded_from_array_elem(e, inside_guard, offsets);
            }
        }
        Expression::Instantiation(inst) => {
            collect_guarded_from_expr(inst.class, inside_guard, offsets);
            if let Some(ref al) = inst.argument_list {
                for arg in al.arguments.iter() {
                    collect_guarded_from_expr(arg.value(), inside_guard, offsets);
                }
            }
        }
        // Don't recurse into closures/arrow functions.
        _ => {}
    }
}

fn collect_guarded_from_array_elem(
    elem: &ArrayElement<'_>,
    inside_guard: bool,
    offsets: &mut HashSet<u32>,
) {
    match elem {
        ArrayElement::KeyValue(kv) => {
            collect_guarded_from_expr(kv.key, inside_guard, offsets);
            collect_guarded_from_expr(kv.value, inside_guard, offsets);
        }
        ArrayElement::Value(v) => {
            collect_guarded_from_expr(v.value, inside_guard, offsets);
        }
        ArrayElement::Variadic(s) => {
            collect_guarded_from_expr(s.value, inside_guard, offsets);
        }
        ArrayElement::Missing(_) => {}
    }
}

/// Collect all variable offsets within an expression that is a target
/// of `isset()` or `empty()`.  This handles simple variables,
/// array access chains (`$arr['key']`), and property chains
/// (`$obj->prop`).
fn collect_guard_targets(expr: &Expression<'_>, offsets: &mut HashSet<u32>) {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            offsets.insert(dv.span().start.offset);
        }
        Expression::ArrayAccess(aa) => {
            collect_guard_targets(aa.array, offsets);
            // Don't mark the index expression as guarded.
        }
        Expression::Access(Access::Property(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::StaticProperty(spa)) => {
            collect_guard_targets(spa.class, offsets);
        }
        _ => {}
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a test backend, open a file, and collect
    /// undefined-variable diagnostics.
    fn collect(php: &str) -> Vec<Diagnostic> {
        let backend = Backend::new_test();
        let uri = "file:///test.php";
        backend.update_ast(uri, php);
        let mut out = Vec::new();
        backend.collect_undefined_variable_diagnostics(uri, php, &mut out);
        out
    }

    // ═══════════════════════════════════════════════════════════════
    // Basic detection
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_undefined_variable_in_echo() {
        let diags = collect(
            r#"<?php
function test(): void {
    echo $nmae;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$nmae"));
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn flags_undefined_variable_in_expression() {
        let diags = collect(
            r#"<?php
function test(): void {
    $x = $y + 1;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$y"));
    }

    #[test]
    fn flags_multiple_undefined_variables() {
        let diags = collect(
            r#"<?php
function test(): void {
    echo $a;
    echo $b;
    echo $c;
}
"#,
        );
        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn diagnostic_has_correct_code_and_source() {
        let diags = collect(
            r#"<?php
function test(): void {
    echo $x;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String("unknown_variable".to_string())),
        );
        assert_eq!(diags[0].source, Some("phpantom".to_string()));
    }

    // ═══════════════════════════════════════════════════════════════
    // Defined variables (no diagnostic)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_assigned_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    $name = "Alice";
    echo $name;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_parameter() {
        let diags = collect(
            r#"<?php
function test(string $name): void {
    echo $name;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_foreach_binding() {
        let diags = collect(
            r#"<?php
function test(array $items): void {
    foreach ($items as $key => $value) {
        echo $key;
        echo $value;
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_foreach_by_reference_binding() {
        let diags = collect(
            r#"<?php
function test(): void {
    $values = [1, 2, 3];
    foreach ($values as &$value) {
        $value = 4;
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_foreach_by_reference_key_value_binding() {
        let diags = collect(
            r#"<?php
function test(array $items): void {
    foreach ($items as $key => &$value) {
        echo $key;
        $value = 'modified';
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_function_param_used_in_catch() {
        let diags = collect(
            r#"<?php
function capture(string $payment, float $amount): void {
    try {
        doSomething($amount);
    } catch (\Exception $e) {
        echo $payment;
        echo $amount;
        echo $e->getMessage();
    }
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Function parameters should be visible inside catch blocks. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_outer_variable_used_in_catch() {
        let diags = collect(
            r#"<?php
function test(): void {
    $client = getClient();
    $token = 'abc';
    try {
        $client->send($token);
    } catch (\RuntimeException $e) {
        log($client, $token, $e->getMessage());
    }
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Variables assigned before try should be visible inside catch blocks. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_try_assigned_variable_used_in_catch() {
        let diags = collect(
            r#"<?php
function test(): void {
    try {
        $response = fetchData();
    } catch (\Exception $e) {
        if (isset($response)) {
            echo $response;
        }
    }
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Variables assigned in try block should be visible in catch (guarded by isset). Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_catch_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    try {
        doSomething();
    } catch (\Exception $e) {
        echo $e->getMessage();
    }
}
"#,
        );
        // $e is defined inside catch, should not flag it.
        // doSomething is an unknown function but not a variable issue.
        assert!(
            !diags.iter().any(|d| d.message.contains("$e")),
            "Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_global_statement() {
        let diags = collect(
            r#"<?php
function test(): void {
    global $config;
    echo $config;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_static_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    static $count = 0;
    echo $count;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_list_destructuring() {
        let diags = collect(
            r#"<?php
function test(array $pair): void {
    [$a, $b] = $pair;
    echo $a;
    echo $b;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_compound_assignment() {
        let diags = collect(
            r#"<?php
function test(): void {
    $x = 0;
    $x += 1;
    echo $x;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_branch_assignment() {
        // Phase 1: any assignment anywhere in the function counts.
        let diags = collect(
            r#"<?php
function test(bool $flag): void {
    if ($flag) {
        $result = "yes";
    }
    echo $result;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Superglobals
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_superglobals() {
        let diags = collect(
            r#"<?php
function test(): void {
    echo $_GET['key'];
    echo $_POST['key'];
    echo $_SERVER['REQUEST_URI'];
    echo $_SESSION['user'];
    echo $_COOKIE['token'];
    echo $_FILES['upload'];
    echo $_ENV['APP_ENV'];
    echo $_REQUEST['data'];
    echo $GLOBALS['x'];
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_argc_argv() {
        let diags = collect(
            r#"<?php
function test(): void {
    echo $argc;
    echo $argv[0];
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // $this
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_this_in_method() {
        let diags = collect(
            r#"<?php
class Foo {
    private string $name;

    public function bar(): string {
        return $this->name;
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_this_in_static_method() {
        // We don't flag $this in static methods — other tools handle that.
        let diags = collect(
            r#"<?php
class Foo {
    public static function bar(): void {
        echo $this;
    }
}
"#,
        );
        assert!(
            !diags.iter().any(|d| d.message.contains("$this")),
            "Got: {:?}",
            diags,
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Suppression: isset / empty
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_isset_guard() {
        let diags = collect(
            r#"<?php
function test(): void {
    if (isset($x)) {
        echo $x;
    }
}
"#,
        );
        // $x inside isset() should not be flagged.
        // $x inside the if body IS defined (by our conservative analysis:
        // any assignment OR isset guard in the function means the name is known).
        // But actually $x is never assigned — so the echo $x should be flagged.
        // However, isset($x) suppresses the read inside isset.
        // The echo $x is still a read of an undefined variable since $x is never assigned.
        assert_eq!(diags.len(), 1, "Got: {:?}", diags);
        // The one diagnostic should be for the echo, not the isset.
        assert!(diags[0].message.contains("$x"));
    }

    #[test]
    fn no_diagnostic_for_empty_guard() {
        let diags = collect(
            r#"<?php
function test(): void {
    if (empty($y)) {
        return;
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Suppression: compact
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_compact_referenced_var() {
        let diags = collect(
            r#"<?php
function test(): void {
    $name = "Alice";
    $age = 30;
    return compact('name', 'age');
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Suppression: extract
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_when_extract_is_used() {
        let diags = collect(
            r#"<?php
function test(array $data): void {
    extract($data);
    echo $name;
    echo $age;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Suppression: variable variables ($$)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_when_variable_variables_are_used() {
        let diags = collect(
            r#"<?php
function test(): void {
    $varName = 'hello';
    $$varName = 'world';
    echo $unknown;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Suppression: @ error control operator
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_error_suppressed_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    echo @$undefined;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Suppression: @var annotation
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_var_annotated_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    /** @var string $name */
    echo $name;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Closures
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_closure_use_captured_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    $x = 42;
    $fn = function() use ($x) {
        echo $x;
    };
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn flags_undefined_in_closure_without_capture() {
        let diags = collect(
            r#"<?php
function test(): void {
    $x = 42;
    $fn = function() {
        echo $x;
    };
}
"#,
        );
        // $x is not captured via use() in the closure, so it should
        // be flagged as undefined inside the closure.
        assert!(
            diags.iter().any(|d| d.message.contains("$x")),
            "Expected undefined $x in closure, got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_closure_parameter() {
        let diags = collect(
            r#"<?php
function test(): void {
    $fn = function(string $name) {
        echo $name;
    };
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Arrow functions
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_arrow_function_implicit_capture() {
        let diags = collect(
            r#"<?php
function test(): void {
    $x = 42;
    $fn = fn() => $x * 2;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    #[test]
    fn no_diagnostic_for_arrow_function_parameter() {
        let diags = collect(
            r#"<?php
function test(): void {
    $fn = fn(int $n) => $n * 2;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Class methods
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_undefined_in_method() {
        let diags = collect(
            r#"<?php
class Foo {
    public function bar(): void {
        echo $undefined;
    }
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$undefined"));
    }

    #[test]
    fn no_diagnostic_for_method_parameter() {
        let diags = collect(
            r#"<?php
class Foo {
    public function bar(string $name): void {
        echo $name;
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Top-level code (should NOT diagnose)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_top_level_code() {
        // Top-level code is skipped because globals are unpredictable.
        let diags = collect(
            r#"<?php
echo $undefined;
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // for loop initialiser
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_for_loop_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    for ($i = 0; $i < 10; $i++) {
        echo $i;
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Unset (should not flag the variable)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_unset_target() {
        let diags = collect(
            r#"<?php
function test(): void {
    $x = 1;
    unset($x);
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Namespaced code
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_undefined_in_namespaced_function() {
        let diags = collect(
            r#"<?php
namespace App;

function test(): void {
    echo $undefined;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$undefined"));
    }

    #[test]
    fn flags_undefined_in_namespaced_class_method() {
        let diags = collect(
            r#"<?php
namespace App;

class Foo {
    public function bar(): void {
        echo $undefined;
    }
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$undefined"));
    }

    // ═══════════════════════════════════════════════════════════════
    // Traits
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_undefined_in_trait_method() {
        let diags = collect(
            r#"<?php
trait MyTrait {
    public function foo(): void {
        echo $undefined;
    }
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$undefined"));
    }

    // ═══════════════════════════════════════════════════════════════
    // Enums
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_undefined_in_enum_method() {
        let diags = collect(
            r#"<?php
enum Status {
    case Active;
    case Inactive;

    public function label(): string {
        return $undefined;
    }
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$undefined"));
    }

    // ═══════════════════════════════════════════════════════════════
    // By-reference parameters
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_reference_parameter() {
        let diags = collect(
            r#"<?php
function test(array &$items): void {
    $items[] = 'new';
    echo count($items);
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Match expression
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_variable_used_in_match() {
        let diags = collect(
            r#"<?php
function test(int $status): string {
    return match($status) {
        1 => 'active',
        2 => 'inactive',
        default => 'unknown',
    };
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // isset with array access (should not flag the root variable)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_isset_with_array_access() {
        let diags = collect(
            r#"<?php
function test(): void {
    if (isset($data['key'])) {
        echo "found";
    }
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // ReadWrite access (e.g. $x++)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_postfix_increment_of_defined_var() {
        let diags = collect(
            r#"<?php
function test(): void {
    $x = 0;
    $x++;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Yield expressions
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn flags_undefined_in_yield() {
        let diags = collect(
            r#"<?php
function test(): \Generator {
    yield $undefined;
}
"#,
        );
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("$undefined"));
    }

    #[test]
    fn no_diagnostic_for_defined_in_yield() {
        let diags = collect(
            r#"<?php
function test(): \Generator {
    $x = 42;
    yield $x;
}
"#,
        );
        assert!(diags.is_empty(), "Got: {:?}", diags);
    }

    // ═══════════════════════════════════════════════════════════════
    // Static property access (self::$prop, static::$prop, etc.)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_self_static_property_access() {
        let diags = collect(
            r#"<?php
class Config {
    private static ?string $instance = null;

    public static function get(): ?string {
        if (self::$instance === null) {
            self::$instance = 'default';
        }
        return self::$instance;
    }
}
"#,
        );
        assert!(
            diags.is_empty(),
            "self::$prop should not be flagged as undefined variable. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_static_keyword_property_access() {
        let diags = collect(
            r#"<?php
class Base {
    protected static int $count = 0;

    public function increment(): void {
        static::$count++;
    }
}
"#,
        );
        assert!(
            diags.is_empty(),
            "static::$prop should not be flagged as undefined variable. Got: {:?}",
            diags,
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // By-reference out-parameters (preg_match, parse_str, etc.)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_preg_match_out_param() {
        let diags = collect(
            r#"<?php
function test(string $input): ?string {
    if (preg_match('/(\d+)/', $input, $match) === 1) {
        return $match[1];
    }
    return null;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "preg_match out-param $match should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_parse_str_out_param() {
        let diags = collect(
            r#"<?php
function test(string $query): string {
    parse_str($query, $data);
    return $data['key'] ?? '';
}
"#,
        );
        assert!(
            diags.is_empty(),
            "parse_str out-param $data should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_preg_match_all_out_param() {
        let diags = collect(
            r#"<?php
function test(string $text): array {
    preg_match_all('/\w+/', $text, $matches);
    return $matches[0];
}
"#,
        );
        assert!(
            diags.is_empty(),
            "preg_match_all out-param $matches should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_fqn_preg_match() {
        let diags = collect(
            r#"<?php
function test(string $input): ?string {
    if (\preg_match('/(\d+)/', $input, $match) === 1) {
        return $match[1];
    }
    return null;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "FQN \\preg_match out-param should be treated as defined. Got: {:?}",
            diags,
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Expanded by-ref out-parameter table — no-diagnostic tests
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_curl_multi_exec_out_param() {
        let diags = collect(
            r#"<?php
function test($mh): int {
    curl_multi_exec($mh, $running);
    return $running;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "curl_multi_exec out-param $running should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_fsockopen_out_params() {
        let diags = collect(
            r#"<?php
function test(): void {
    $fp = fsockopen('example.com', 80, $errno, $errstr);
    echo $errno . $errstr;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "fsockopen out-params $errno/$errstr should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_openssl_sign_out_param() {
        let diags = collect(
            r#"<?php
function test(string $data, $key): string {
    openssl_sign($data, $signature, $key);
    return $signature;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "openssl_sign out-param $signature should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_getimagesize_out_param() {
        let diags = collect(
            r#"<?php
function test(string $file): array {
    $info = getimagesize($file, $imageinfo);
    return $imageinfo;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "getimagesize out-param $imageinfo should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_headers_sent_out_params() {
        let diags = collect(
            r#"<?php
function test(): void {
    headers_sent($file, $line);
    echo $file . ':' . $line;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "headers_sent out-params $file/$line should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_pcntl_wait_out_param() {
        let diags = collect(
            r#"<?php
function test(): void {
    pcntl_wait($status);
    echo $status;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "pcntl_wait out-param $status should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_dns_get_mx_out_params() {
        let diags = collect(
            r#"<?php
function test(string $host): void {
    dns_get_mx($host, $mxhosts, $weights);
    var_dump($mxhosts, $weights);
}
"#,
        );
        assert!(
            diags.is_empty(),
            "dns_get_mx out-params should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_flock_out_param() {
        let diags = collect(
            r#"<?php
function test($fp): void {
    flock($fp, LOCK_EX, $wouldblock);
    echo $wouldblock;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "flock out-param $wouldblock should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_mb_parse_str_out_param() {
        let diags = collect(
            r#"<?php
function test(string $input): array {
    mb_parse_str($input, $result);
    return $result;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "mb_parse_str out-param $result should be treated as defined. Got: {:?}",
            diags,
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Generic by-ref detection via resolver (user-defined functions,
    // static methods, constructors)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_user_defined_function_byref_param() {
        let php = r#"<?php
function myFunc(string $input, array &$output): void {
    $output = [$input];
}
function test(string $val): void {
    myFunc($val, $result);
    echo $result[0];
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "User-defined function by-ref $result should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_static_method_byref_param() {
        let php = r#"<?php
class Validator {
    public static function validate(string $input, array &$errors): bool {
        $errors = [];
        return true;
    }
}
function test(string $data): void {
    Validator::validate($data, $errors);
    var_dump($errors);
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Static method by-ref $errors should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_constructor_byref_param() {
        let php = r#"<?php
class Parser {
    public function __construct(string $input, array &$warnings) {
        $warnings = [];
    }
}
function test(string $src): void {
    $p = new Parser($src, $warnings);
    var_dump($warnings);
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Constructor by-ref $warnings should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_namespaced_unqualified_function_byref_param() {
        let php = r#"<?php
namespace App;
function initItem(?string &$out): void {
    $out = 'hello';
}
class Svc {
    public function demo(): void {
        initItem($result);
        echo $result;
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Unqualified call to namespaced function with by-ref param should not flag $result. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_namespaced_static_method_byref_param() {
        let php = r#"<?php
namespace App;
class Factory {
    public static function create(?string &$out): void {
        $out = 'hello';
    }
}
class Svc {
    public function demo(): void {
        Factory::create($result);
        echo $result;
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Static method with by-ref param in namespace should not flag $result. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_namespaced_constructor_byref_param() {
        let php = r#"<?php
namespace App;
class Builder {
    public function __construct(?string &$out) {
        $out = 'built';
    }
}
class Svc {
    public function demo(): void {
        new Builder($result);
        echo $result;
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "Constructor with by-ref param in namespace should not flag $result. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_this_method_byref_param() {
        let php = r#"<?php
class Svc {
    private function init(?string &$out): void {
        $out = 'ready';
    }
    public function demo(): void {
        $this->init($result);
        echo $result;
    }
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "$this->method() with by-ref param should not flag $result. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_fqn_user_defined_function_byref_param() {
        let php = r#"<?php
namespace App;
function transform(string $in, array &$out): void {
    $out = [$in];
}
function test(): void {
    \App\transform('hello', $result);
    echo $result[0];
}
"#;
        let diags = collect(php);
        assert!(
            diags.is_empty(),
            "FQN user-defined function by-ref $result should be treated as defined. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn diagnostic_still_fires_for_truly_undefined_after_non_byref_call() {
        let php = r#"<?php
function noRefs(string $a): void {}
function test(): void {
    noRefs('hello');
    echo $undefined;
}
"#;
        let diags = collect(php);
        assert_eq!(
            diags.len(),
            1,
            "Should flag $undefined even when resolver is active. Got: {:?}",
            diags,
        );
        assert!(
            diags[0].message.contains("$undefined"),
            "Diagnostic should be for $undefined",
        );
    }

    // ═══════════════════════════════════════════════════════════════
    // Multi-level nesting (arrow fn in closure, catch in closure)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn no_diagnostic_for_arrow_fn_capturing_closure_variable() {
        let diags = collect(
            r#"<?php
function test(): void {
    $callback = function (array $ids) {
        $sortMap = array_flip($ids);
        return array_map(fn($item) => $sortMap[$item], $ids);
    };
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Arrow fn should see variables from enclosing closure. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_nested_closure_use_captures() {
        let diags = collect(
            r#"<?php
function test(): void {
    $outer = function () {
        $brandIds = [1, 2, 3];
        $typeIds = [4, 5, 6];

        $inner = function () use ($brandIds, $typeIds) {
            return [$brandIds[0], $typeIds[0]];
        };

        return $inner();
    };
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Nested closure use() captures should be visible. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_catch_inside_closure() {
        let diags = collect(
            r#"<?php
function test(): void {
    $handler = function (string $payment) {
        $client = getClient();
        try {
            $response = $client->send($payment);
        } catch (\Exception $e) {
            log($payment, $client, $e->getMessage());
        }
    };
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Catch inside closure should see closure variables. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_variable_assigned_in_try_used_in_catch_inside_closure() {
        let diags = collect(
            r#"<?php
function test(): void {
    $handler = function () {
        $fullFilePath = '/tmp/test.jpg';
        try {
            process($fullFilePath);
        } catch (\Throwable $e) {
            fallback($fullFilePath);
        }
    };
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Variable assigned before try should be visible in catch inside closure. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_arrow_fn_in_closure_in_method() {
        let diags = collect(
            r#"<?php
class Foo {
    public function run(): void {
        $this->process(function (array $products, array $ids) {
            $sortMap = array_flip($ids);
            return $products->sortBy(fn($product) => $sortMap[$product->id]);
        });
    }
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Arrow fn in closure in method should see closure variables. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_deeply_nested_arrow_functions() {
        let diags = collect(
            r#"<?php
function test(): void {
    $a = 1;
    $f = function () use ($a) {
        $b = 2;
        $g = fn() => fn() => $a + $b;
        return $g;
    };
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Deeply nested arrow fns should see all ancestor variables. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn flags_undefined_in_closure_without_capture_nested() {
        let diags = collect(
            r#"<?php
function test(): void {
    $outer = function () {
        $local = 42;
        $inner = function () {
            echo $local;
        };
    };
}
"#,
        );
        assert_eq!(
            diags.len(),
            1,
            "Closure without use() should not see parent closure variables. Got: {:?}",
            diags,
        );
        assert!(diags[0].message.contains("$local"));
    }

    #[test]
    fn no_diagnostic_for_array_access_assignment() {
        let diags = collect(
            r#"<?php
function test(): void {
    $b['a'] = 'hello';
    echo $b;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Array access assignment should define the variable. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_nested_array_access_assignment() {
        let diags = collect(
            r#"<?php
function test(): void {
    $b['a']['a'] = 'a';
    echo $b;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Nested array access assignment should define the variable. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_deeply_nested_array_access_assignment() {
        let diags = collect(
            r#"<?php
function test(): void {
    $config['db']['host']['primary'] = 'localhost';
    echo $config;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Deeply nested array access assignment should define the variable. Got: {:?}",
            diags,
        );
    }

    #[test]
    fn no_diagnostic_for_array_append_assignment() {
        let diags = collect(
            r#"<?php
function test(): void {
    $items[] = 'hello';
    echo $items;
}
"#,
        );
        assert!(
            diags.is_empty(),
            "Array append assignment should define the variable. Got: {:?}",
            diags,
        );
    }
}
