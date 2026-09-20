//! Unused variable diagnostics.
//!
//! Catch variables are only flagged when the target PHP version is >= 8.0,
//! because prior versions require the variable in `catch` syntax (there is
//! no way to omit it).
//!
//! Flag variables that are assigned (or declared as parameters) but
//! never read in the same scope.  This catches dead code, typos in
//! variable names, and forgotten refactoring leftovers.
//!
//! Severity: `Hint` with `DiagnosticTag::Unnecessary` so editors
//! render unused variables as dimmed/faded text.

use std::collections::{HashMap, HashSet};

use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use super::helpers::make_tagged_diagnostic;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::diagnostics::undefined_variables::{
    collect_compact_vars, collect_import_offsets, has_get_defined_vars,
};
use crate::parser::with_parsed_program;
use crate::scope_collector::{
    AccessKind, FrameKind, ScopeBody, ScopeMap, collect_function_scope_with_kind,
    collect_function_scope_with_resolver,
};
use crate::types::PhpVersion;

/// Diagnostic code used for unused-variable diagnostics.
pub(crate) const UNUSED_VARIABLE_CODE: &str = "unused_variable";

/// PHP superglobals that should never be flagged.
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
    /// Collect unused-variable diagnostics for a single file.
    pub fn collect_unused_variable_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if self.should_skip_diagnostics(uri) {
            return;
        }

        let php_version = self.php_version();

        with_parsed_program(content, "unused_variable", |program, content| {
            let mut ctx = DiagnosticCtx {
                backend: self,
                uri,
                content,
                php_version,
                diagnostics: Vec::new(),
            };

            let walker = UnusedVariableWalker;
            for stmt in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, stmt, &mut ctx);
            }

            out.extend(ctx.diagnostics);
        });
    }
}

// ─── Internal context ───────────────────────────────────────────────────────

struct DiagnosticCtx<'a> {
    backend: &'a Backend,
    uri: &'a str,
    content: &'a str,
    php_version: PhpVersion,
    diagnostics: Vec<Diagnostic>,
}

// ─── AST walking ────────────────────────────────────────────────────────────

/// Walker that runs unused-variable analysis on every function and method
/// body it encounters. The generated traversal reaches bodies at any depth
/// (top-level, class members, nested functions, anonymous-class methods),
/// so no node kind needs an explicit dispatch arm. Closures and arrow
/// functions are not visited as their own scopes here; their frames are
/// analysed as part of the enclosing function/method scope by the scope
/// collector.
struct UnusedVariableWalker;

impl<'ast, 'arena, 'a> mago_syntax::walker::Walker<'ast, 'arena, DiagnosticCtx<'a>>
    for UnusedVariableWalker
{
    fn walk_in_function(&self, func: &'ast Function<'arena>, ctx: &mut DiagnosticCtx<'a>) {
        let body_start = func.body.left_brace.start.offset;
        let body_end = func.body.right_brace.end.offset;
        let body = ScopeBody::Statements(func.body.statements.as_slice());
        let compact_vars = collect_compact_vars(body);
        let has_get_defined = has_get_defined_vars(body);
        let imports = collect_import_offsets(body);
        let scope = collect_function_scope_with_resolver(
            &func.parameter_list,
            func.body.statements.as_slice(),
            body_start,
            body_end,
            None,
        );
        check_scope(&scope, ctx, None, &compact_vars, has_get_defined, &imports);
    }

    fn walk_in_method(&self, method: &'ast Method<'arena>, ctx: &mut DiagnosticCtx<'a>) {
        let MethodBody::Concrete(block) = &method.body else {
            return;
        };
        let body_start = block.left_brace.start.offset;
        let body_end = block.right_brace.end.offset;

        // Collect promoted parameter names so we can exclude them.
        let promoted_params = collect_promoted_params(&method.parameter_list);

        let body = ScopeBody::Statements(block.statements.as_slice());
        let compact_vars = collect_compact_vars(body);
        let has_get_defined = has_get_defined_vars(body);
        let imports = collect_import_offsets(body);
        let scope = collect_function_scope_with_kind(
            &method.parameter_list,
            block.statements.as_slice(),
            body_start,
            body_end,
            FrameKind::Method,
        );

        check_scope(
            &scope,
            ctx,
            Some(&promoted_params),
            &compact_vars,
            has_get_defined,
            &imports,
        );
    }
}

/// Collect names of promoted constructor parameters (those with a
/// visibility modifier like `public`, `protected`, `private`).
/// These become class properties and are never "unused".
fn collect_promoted_params(params: &FunctionLikeParameterList<'_>) -> HashSet<String> {
    let mut promoted = HashSet::new();
    for param in params.parameters.iter() {
        if param.is_promoted_property() {
            promoted.insert(bytes_to_str(param.variable.name).to_string());
        }
    }
    promoted
}

// ─── Scope analysis ────────────────────────────────────────────────────────

/// Check a single scope for unused variables.
///
/// `promoted_params` is `Some` when checking a method — it lists
/// constructor promoted parameter names that should be skipped.
///
/// `import_offsets` holds the start offset of every `include`/`require` in
/// the body; a frame containing one is left alone entirely (see
/// [`frame_owns_import`]).
fn check_scope(
    scope: &ScopeMap,
    ctx: &mut DiagnosticCtx<'_>,
    promoted_params: Option<&HashSet<String>>,
    compact_vars: &HashSet<String>,
    has_get_defined_vars: bool,
    import_offsets: &[u32],
) {
    if scope.frames.is_empty() {
        return;
    }

    let always_skip: HashSet<&str> = {
        let mut set: HashSet<&str> = HashSet::new();
        for sg in SUPERGLOBALS {
            set.insert(sg);
        }
        set.insert("$this");
        set
    };

    // Nested frames (closures, arrow functions, catch blocks) have their
    // own scope, so writes inside them must not be counted against this
    // frame.  Closure and arrow function parameters are written at
    // offsets that fall inside the parent frame but before the child's
    // body starts, so the parent must not claim those as its own writes
    // either.
    for frame in scope.frames.iter() {
        // Skip top-level frames — global scope has too many implicit defs.
        if frame.kind == FrameKind::TopLevel {
            continue;
        }

        // `get_defined_vars()` only consumes variables from the enclosing
        // function/method scope. Nested closures, arrow functions, and catch
        // blocks still need their own unused-variable analysis.
        if has_get_defined_vars && matches!(frame.kind, FrameKind::Function | FrameKind::Method) {
            continue;
        }

        // An `include`/`require` runs the target file in this frame's own
        // variable scope, so any local here can be read from a file this
        // analysis never sees.
        if import_offsets
            .iter()
            .any(|&offset| frame_owns_import(offset, frame, &scope.frames))
        {
            continue;
        }

        // For catch frames, we only check the catch variable (which is
        // in frame.parameters).  We don't re-check variables inherited
        // from the parent — those are the parent frame's responsibility.
        // This avoids duplicate diagnostics.
        if frame.kind == FrameKind::Catch {
            // Before PHP 8.0, catch variables are mandatory syntax —
            // there is no way to omit them, so flagging them is noise.
            if ctx.php_version >= PhpVersion::new(8, 0) {
                check_catch_frame(frame, scope, ctx, &always_skip);
            }
            continue;
        }

        // Collect all variables written in this frame (directly, not
        // inside nested frames that create their own scope).
        let mut written_vars: HashMap<&str, u32> = HashMap::new();

        for access in &scope.accesses {
            if !matches!(access.kind, AccessKind::Write | AccessKind::ReadWrite) {
                continue;
            }
            if access.offset < frame.start || access.offset > frame.end {
                continue;
            }
            // Skip writes inside nested frames (closures, arrow fns,
            // catch blocks) — those belong to the child scope.
            if is_in_nested_frame(access.offset, frame, &scope.frames) {
                continue;
            }
            // Skip writes that are catch variable declarations — these
            // are checked separately by check_catch_frame and must not
            // be double-counted as regular writes in the parent scope.
            if is_catch_frame_parameter(access.name.as_str(), access.offset, frame, &scope.frames) {
                continue;
            }
            // Skip writes that are actually parameters of a nested
            // closure or arrow function.  Their parameter declaration
            // offset falls between the parent frame.start and the
            // child frame.start, so is_in_nested_frame doesn't catch them.
            if is_nested_frame_parameter(access.name.as_str(), access.offset, frame, &scope.frames)
            {
                continue;
            }
            written_vars
                .entry(access.name.as_str())
                .or_insert(access.offset);
        }

        // Also record this frame's own parameters as written.
        for param in &frame.parameters {
            if !written_vars.contains_key(param.as_str()) {
                let offset = scope
                    .accesses
                    .iter()
                    .find(|a| a.name == param.as_str() && matches!(a.kind, AccessKind::Write))
                    .map(|a| a.offset)
                    .unwrap_or(frame.start);
                written_vars.insert(param.as_str(), offset);
            }
        }

        // For each written variable, check if it has any reads.
        for (&var_name, &write_offset) in &written_vars {
            if always_skip.contains(var_name) {
                continue;
            }

            if var_name == "$_" || var_name.starts_with("$_") {
                continue;
            }

            // Skip variables captured by reference (`use (&$var)`).
            // A write to a by-reference capture inside the closure
            // propagates to the outer scope, so the variable is never
            // truly unused within the closure frame even if it is only
            // written (and never read) here.
            if frame
                .captures
                .iter()
                .any(|(name, by_ref)| *by_ref && name == var_name)
            {
                continue;
            }

            // Skip the variables the Blade preprocessor synthesizes:
            // `$loop` for every @foreach/@forelse, `$component` for every
            // component tag, and the ones a component tag binds its
            // arguments to.  None is written by the author, and a
            // template is under no obligation to read them.
            if crate::blade::is_blade_file(ctx.uri) {
                let bare_name = var_name.trim_start_matches('$');
                if bare_name == "loop"
                    || bare_name == crate::blade::COMPONENT_VAR
                    || bare_name.starts_with(crate::blade::preprocessor::ARGUMENT_VAR_PREFIX)
                {
                    continue;
                }
            }

            // Skip variables referenced by compact().
            if compact_vars.contains(var_name) {
                continue;
            }

            // Skip promoted constructor parameters.
            if let Some(promoted) = promoted_params
                && promoted.contains(var_name)
            {
                continue;
            }

            // Check for reads in the frame scope (including nested arrow
            // functions and catch blocks, but NOT nested closures).
            if has_reads_in_scope(var_name, frame, scope) {
                continue;
            }

            let is_parameter = frame.parameters.iter().any(|p| p.as_str() == var_name);

            // Skip parameters entirely for now — flagging unused parameters
            // is unsafe without suppression support because callbacks, interface
            // implementations, and framework conventions often require specific
            // signatures even when not all parameters are used.
            if is_parameter {
                continue;
            }

            // Determine the offset for the diagnostic.
            let var_len = var_name.len();
            let range = match ctx.backend.offset_range_to_lsp_range(
                ctx.uri,
                ctx.content,
                write_offset as usize,
                write_offset as usize + var_len,
            ) {
                Some(r) => r,
                None => continue,
            };

            let message = format!("Unused variable '{}'", var_name);

            ctx.diagnostics.push(make_tagged_diagnostic(
                range,
                DiagnosticSeverity::HINT,
                UNUSED_VARIABLE_CODE,
                message,
                Some(DiagnosticTag::UNNECESSARY),
            ));
        }
    }
}

/// Check a catch frame for unused catch variables only.
///
/// Catch frames inherit the parent scope, so we only flag variables
/// that are in the catch frame's own `parameters` list (the catch
/// variable itself, e.g. `$e` in `catch (Exception $e)`).
fn check_catch_frame(
    frame: &crate::scope_collector::Frame,
    scope: &ScopeMap,
    ctx: &mut DiagnosticCtx<'_>,
    always_skip: &HashSet<&str>,
) {
    for param in &frame.parameters {
        let var_name = param.as_str();

        if always_skip.contains(var_name) {
            continue;
        }
        if var_name == "$_" || var_name.starts_with("$_") {
            continue;
        }

        // Check for reads inside the catch block body.
        let has_read = scope.accesses.iter().any(|a| {
            a.name == var_name
                && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
                && a.offset >= frame.start
                && a.offset <= frame.end
        });

        if has_read {
            continue;
        }

        // Find the catch variable's write offset — the closest write
        // *before* this frame's start.  Using `.find()` would return the
        // first write in the file, which for nested catches with the same
        // variable name points at the wrong catch clause.
        let diag_offset = scope
            .accesses
            .iter()
            .filter(|a| {
                a.name == var_name && matches!(a.kind, AccessKind::Write) && a.offset <= frame.start
            })
            .max_by_key(|a| a.offset)
            .or_else(|| {
                scope
                    .accesses
                    .iter()
                    .find(|a| a.name == var_name && matches!(a.kind, AccessKind::Write))
            })
            .map(|a| a.offset)
            .unwrap_or(frame.start);

        let var_len = var_name.len();
        let range = match ctx.backend.offset_range_to_lsp_range(
            ctx.uri,
            ctx.content,
            diag_offset as usize,
            diag_offset as usize + var_len,
        ) {
            Some(r) => r,
            None => continue,
        };

        ctx.diagnostics.push(make_tagged_diagnostic(
            range,
            DiagnosticSeverity::HINT,
            UNUSED_VARIABLE_CODE,
            format!("Unused variable '{}'", var_name),
            Some(DiagnosticTag::UNNECESSARY),
        ));
    }
}

/// Check whether the variable `name` has any read accesses within the
/// given frame, including reads inside nested arrow functions and catch
/// blocks (which inherit the parent scope), but excluding reads inside
/// nested closures (which have their own scope).
fn has_reads_in_scope(name: &str, frame: &crate::scope_collector::Frame, scope: &ScopeMap) -> bool {
    scope.accesses.iter().any(|a| {
        a.name == name
            && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
            && a.offset >= frame.start
            && a.offset <= frame.end
            && !is_in_nested_closure(a.offset, frame, &scope.frames)
    })
}

/// Check whether the `include`/`require` at `offset` runs in `frame`'s own
/// variable scope.
///
/// Catch blocks share the enclosing scope, so an import inside one exposes
/// the enclosing frame's locals too; a closure, arrow function, or named
/// function nested in between gets its own scope, which is where the import
/// stops.
fn frame_owns_import(
    offset: u32,
    frame: &crate::scope_collector::Frame,
    frames: &[crate::scope_collector::Frame],
) -> bool {
    if offset < frame.start || offset > frame.end {
        return false;
    }
    !frames.iter().any(|f| {
        f.start > frame.start
            && f.end < frame.end
            && offset >= f.start
            && offset <= f.end
            && matches!(
                f.kind,
                FrameKind::Closure | FrameKind::ArrowFunction | FrameKind::Function
            )
    })
}

/// Check whether `offset` falls inside a nested closure frame (but not
/// an arrow function or catch frame) within the given parent frame.
fn is_in_nested_closure(
    offset: u32,
    parent: &crate::scope_collector::Frame,
    frames: &[crate::scope_collector::Frame],
) -> bool {
    frames.iter().any(|f| {
        f.start > parent.start
            && f.end < parent.end
            && offset >= f.start
            && offset <= f.end
            && f.kind == FrameKind::Closure
    })
}

/// Check whether a variable at the given offset is a catch frame's
/// parameter declaration within the given parent frame.
fn is_catch_frame_parameter(
    name: &str,
    offset: u32,
    parent: &crate::scope_collector::Frame,
    frames: &[crate::scope_collector::Frame],
) -> bool {
    frames.iter().any(|f| {
        f.kind == FrameKind::Catch
            && f.start > parent.start
            && f.end < parent.end
            && f.parameters.iter().any(|p| p.as_str() == name)
            && offset < f.start
    })
}

/// Check whether `offset` falls inside any nested frame (closure,
/// arrow function, or catch) within the given parent frame.
fn is_in_nested_frame(
    offset: u32,
    parent: &crate::scope_collector::Frame,
    frames: &[crate::scope_collector::Frame],
) -> bool {
    frames.iter().any(|f| {
        f.start > parent.start && f.end < parent.end && offset >= f.start && offset <= f.end
    })
}

/// Check whether a write access at `offset` for variable `name` is
/// actually a parameter declaration of a nested closure or arrow
/// function frame.
///
/// Closure and arrow function parameters are declared at offsets that
/// fall between `parent.start` and `child.start` — the parent frame
/// body contains them, but `is_in_nested_frame` does not catch them
/// because the child frame's body hasn't started yet.  We must not
/// let the parent frame claim these parameter writes as its own.
fn is_nested_frame_parameter(
    name: &str,
    offset: u32,
    parent: &crate::scope_collector::Frame,
    frames: &[crate::scope_collector::Frame],
) -> bool {
    frames.iter().any(|f| {
        // Only consider frames nested within the parent.
        f.start > parent.start
            && f.end < parent.end
            // The parameter declaration is between the parent body start
            // and the child body start (e.g. in the parameter list of a
            // closure or arrow function).
            && offset < f.start
            && offset > parent.start
            // Check that this variable is actually a parameter of this frame.
            && f.parameters.iter().any(|p| p.as_str() == name)
    })
}
