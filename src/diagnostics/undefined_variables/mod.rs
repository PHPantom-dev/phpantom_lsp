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

use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::parser::with_parsed_program;
use crate::scope_collector::{
    AccessKind, ByRefCallKind, ByRefResolver, FrameKind, ScopeMap,
    collect_function_scope_with_kind_and_resolver, collect_function_scope_with_resolver,
};

use super::helpers::make_diagnostic;

mod feature_guards;
mod offset_guards;

pub(crate) use feature_guards::{collect_compact_vars, has_get_defined_vars};
use feature_guards::{has_dynamic_variables, has_extract_call};
use offset_guards::{
    collect_error_suppressed_offsets, collect_guarded_offsets, collect_var_annotations,
};

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
