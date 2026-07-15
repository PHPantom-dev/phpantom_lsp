//! ScopeCollector — forward-pass variable read/write analysis.
//!
//! This module provides a lightweight forward-pass AST walker that
//! collects every variable read and write with byte offsets across a
//! function/method/closure body.  It is shared infrastructure used by:
//!
//! - Extract Function
//! - Inline Variable
//! - Extract Variable
//! - Inline Function/Method
//! - Extract Constant
//! - Undefined variable diagnostic
//! - Document highlights (all occurrences of a variable in scope)
//!
//! Unlike the existing backward-walk variable resolution in
//! `completion/variable/resolution.rs` (which resolves the type of a
//! single variable at a specific cursor position), the `ScopeCollector`
//! walks **forward** through an entire function body and records _all_
//! variable definitions and usages.
//!
//! # Key concepts
//!
//! - **Frame** = scope boundary.  Each function body, closure, arrow
//!   function, and `catch` block opens a new frame.  Variables defined
//!   inside a frame are local to it.  Closures capture via `use()`;
//!   arrow functions capture by value.  `foreach`, `if`, `for` blocks
//!   do _not_ open new frames in PHP — variables leak into the
//!   enclosing scope.
//!
//! - **VarAccess** = a single read or write of a variable, with name,
//!   byte offset, and access kind.
//!
//! - **ScopeMap** = the result of collecting.  Contains all accesses
//!   organised by frame, plus a query API for extracting parameter sets,
//!   return value sets, and local sets for a given byte range.

#[cfg(test)]
mod tests;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;

// ─── By-reference parameter resolution ──────────────────────────────────────

/// Describes a call expression so the by-ref resolver can look up
/// the callee's parameter list.
pub(crate) enum ByRefCallKind<'a> {
    /// A standalone function call (e.g. `myFunc($var)`).
    Function(&'a str),
    /// A static method call (e.g. `Cls::method($var)`).
    StaticMethod(&'a str, &'a str),
    /// A constructor call (e.g. `new Cls($var)`).
    Constructor(&'a str),
    /// An instance method call on `$this` (e.g. `$this->method($var)`).
    /// The class name is the enclosing class at the call site.
    InstanceMethod(&'a str, &'a str),
}

/// Callback that resolves by-reference parameter positions for a call.
///
/// Given a [`ByRefCallKind`] describing the call, returns a list of
/// 0-based argument positions that are by-reference.  Returns `None`
/// if the function/method cannot be resolved.
pub(crate) type ByRefResolver<'a> = &'a dyn Fn(&ByRefCallKind<'_>) -> Option<Vec<usize>>;

// ─── Core types ─────────────────────────────────────────────────────────────

/// Whether a variable access is a read or a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessKind {
    /// The variable is being read (e.g. `foo($x)`, `return $x`).
    Read,
    /// The variable is being written (e.g. `$x = …`, parameter decl,
    /// foreach binding, catch binding).
    Write,
    /// The variable is being both read and written (e.g. `$x .= …`,
    /// `$x++`, `$x--`, `$x += …`).
    ReadWrite,
}

/// A single variable access (read or write) at a specific byte offset.
#[derive(Debug, Clone)]
pub(crate) struct VarAccess {
    /// Variable name **with** `$` prefix (e.g. `$x`, `$this`).
    pub name: String,
    /// Byte offset of the `$` character in the source.
    pub offset: u32,
    /// Whether this is a read, write, or read-write access.
    pub kind: AccessKind,
}

/// A scope frame representing a function, closure, or arrow function body.
///
/// Each frame records its own variable accesses.  Frames form a tree
/// (closures nested inside functions, etc.), but for the initial
/// implementation we store them in a flat vec and use byte-range
/// containment to determine nesting.
#[derive(Debug, Clone)]
pub(crate) struct Frame {
    /// Byte offset of the frame's opening boundary.
    ///
    /// For functions/methods: the opening `{` of the body.
    /// For closures: the opening `{` of the body.
    /// For arrow functions: the `=>` token offset.
    /// For catch blocks: the opening `{` of the catch body.
    /// For top-level code: `0`.
    pub start: u32,
    /// Byte offset of the frame's closing boundary.
    ///
    /// For functions/methods/closures/catch: the closing `}`.
    /// For arrow functions: the end of the body expression.
    /// For top-level code: `u32::MAX`.
    pub end: u32,
    /// What kind of scope boundary this frame represents.
    pub kind: FrameKind,
    /// Variables explicitly captured via `use($x, &$y)` in closures.
    /// Each entry is `(name_with_dollar, is_by_reference)`.
    ///
    /// Populated during collection; read by the unused-variable
    /// diagnostic to skip by-reference captures, and by Extract Function
    /// to detect closure captures that cross extraction boundaries.
    pub captures: Vec<(String, bool)>,
    /// Parameter names (with `$` prefix) declared on this frame.
    ///
    /// Populated for functions, methods, closures, and arrow functions.
    /// Used by the undefined-variable diagnostic to identify which
    /// variables are defined as parameters (their write offsets are
    /// before the frame's `start` and cannot be distinguished from
    /// outer-scope writes by offset alone).
    pub parameters: Vec<String>,
}

/// The kind of scope boundary a [`Frame`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    /// Top-level code (outside any function/class).
    TopLevel,
    /// Named function: `function foo() { … }`
    Function,
    /// Class method (regular, static, abstract with body, etc.).
    Method,
    /// Closure: `function($x) use($y) { … }`
    Closure,
    /// Arrow function: `fn($x) => expr`
    ArrowFunction,
    /// Catch block: `catch (E $e) { … }`
    Catch,
}

/// The result of a scope collection pass.
///
/// Contains all variable accesses organised by frame, plus a query API
/// for extracting parameter / return-value / local sets for a given
/// byte range.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScopeMap {
    /// All variable accesses across all frames, in source order.
    pub accesses: Vec<VarAccess>,
    /// All scope frames, sorted by `start` offset.
    pub frames: Vec<Frame>,
    /// Whether `$this`, `self::`, `static::`, or `parent::` appears
    /// anywhere in the collected region.  Set during collection.
    pub has_this_or_self: bool,
    /// Whether any by-reference parameter (`&$var`) was encountered.
    ///
    /// Used by Extract Function to detect when by-reference semantics
    /// would make extraction unsafe.
    pub has_reference_params: bool,
}

impl ScopeMap {
    /// Whether the enclosing scope uses by-reference parameters.
    ///
    /// When `true`, variable extraction must be careful about
    /// reference semantics — a variable modified via `&$var` in the
    /// extracted range may need to be passed by reference to the new
    /// function.
    pub(crate) fn uses_reference_params(&self) -> bool {
        self.has_reference_params
    }
}

/// Variables classified by their role relative to a byte range.
///
/// Returned by [`ScopeMap::classify_range`].
#[derive(Debug, Clone, Default)]
pub(crate) struct RangeClassification {
    /// Variables **read** inside `[start, end)` whose most recent
    /// write is **before** `start`.  These would become parameters
    /// of an extracted function.
    pub parameters: Vec<String>,
    /// Variables **written** inside `[start, end)` that are **read
    /// after** `end` in the enclosing scope.  These would become
    /// return values of an extracted function.
    pub return_values: Vec<String>,
    /// Variables whose entire lifetime (first write to last read) is
    /// contained within `[start, end)`.  These stay inside the
    /// extracted function.
    pub locals: Vec<String>,
    /// Whether `$this`, `self::`, `static::`, or `parent::` appears
    /// in the range.
    pub uses_this: bool,
    /// Variables that are written by reference (`&$var`) inside the
    /// range.
    pub reference_writes: Vec<String>,
}

// ─── ScopeMap query API ─────────────────────────────────────────────────────

impl ScopeMap {
    /// Find the innermost frame that fully contains the given offset.
    pub(crate) fn enclosing_frame(&self, offset: u32) -> Option<&Frame> {
        // Iterate in reverse so we find the innermost (most recently
        // opened) frame first.  Frames are sorted by start offset.
        self.frames
            .iter()
            .rev()
            .find(|f| offset >= f.start && offset <= f.end)
    }

    /// Find the innermost frame that fully contains the given range.
    pub(crate) fn enclosing_frame_for_range(&self, start: u32, end: u32) -> Option<&Frame> {
        self.frames
            .iter()
            .rev()
            .find(|f| start >= f.start && end <= f.end)
    }

    /// Return all accesses of variable `name` within the given frame,
    /// excluding accesses that fall inside a nested frame (closure or
    /// arrow function).
    pub(crate) fn accesses_in_frame<'a>(&'a self, name: &str, frame: &Frame) -> Vec<&'a VarAccess> {
        self.accesses
            .iter()
            .filter(|a| a.name == name && a.offset >= frame.start && a.offset <= frame.end)
            .filter(|a| {
                !self.frames.iter().any(|f| {
                    f.start > frame.start
                        && f.end < frame.end
                        && a.offset >= f.start
                        && a.offset <= f.end
                        && f.kind != FrameKind::Catch
                })
            })
            .collect()
    }

    /// Classify variables relative to a byte range `[start, end)`.
    ///
    /// This is the primary query for Extract Function: it determines
    /// which variables become parameters, return values, or locals.
    pub(crate) fn classify_range(&self, start: u32, end: u32) -> RangeClassification {
        let frame = match self.enclosing_frame_for_range(start, end) {
            Some(f) => f,
            None => return RangeClassification::default(),
        };

        // Collect all unique variable names accessed within the range
        // (excluding nested frames and pseudo-variables).
        let mut var_names: Vec<String> = Vec::new();
        for access in &self.accesses {
            if access.offset >= start
                && access.offset < end
                && !var_names.contains(&access.name)
                && access.name != "$this"
                && access.name != "self"
                && access.name != "static"
                && access.name != "parent"
            {
                // Skip if inside a nested frame.
                let in_nested = self.frames.iter().any(|f| {
                    f.start > frame.start
                        && f.end < frame.end
                        && access.offset >= f.start
                        && access.offset <= f.end
                        && f.kind != FrameKind::Catch
                });
                if !in_nested {
                    var_names.push(access.name.clone());
                }
            }
        }

        // Check for $this / self / static / parent usage in range.
        let mut result = RangeClassification {
            uses_this: self.accesses.iter().any(|a| {
                a.offset >= start
                    && a.offset < end
                    && (a.name == "$this"
                        || a.name == "self"
                        || a.name == "static"
                        || a.name == "parent")
            }),
            ..Default::default()
        };

        for var_name in &var_names {
            let frame_accesses = self.accesses_in_frame(var_name, frame);

            let has_write_before = frame_accesses.iter().any(|a| {
                a.offset < start && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
            });

            let has_read_inside = frame_accesses.iter().any(|a| {
                a.offset >= start
                    && a.offset < end
                    && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
            });

            let has_write_inside = frame_accesses.iter().any(|a| {
                a.offset >= start
                    && a.offset < end
                    && matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite)
            });

            let has_read_after = frame_accesses.iter().any(|a| {
                a.offset >= end && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
            });

            let first_write = frame_accesses
                .iter()
                .filter(|a| matches!(a.kind, AccessKind::Write | AccessKind::ReadWrite))
                .min_by_key(|a| a.offset);

            let last_read = frame_accesses
                .iter()
                .filter(|a| matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite))
                .max_by_key(|a| a.offset);

            // Variable whose entire lifetime is within [start, end).
            let entirely_inside = first_write.is_some_and(|w| w.offset >= start && w.offset < end)
                && last_read.is_none_or(|r| r.offset < end)
                && !has_write_before
                && !has_read_after;

            if entirely_inside {
                result.locals.push(var_name.clone());
            } else if has_read_inside && has_write_before && !has_write_inside {
                // Read-only inside the range, written before → parameter.
                result.parameters.push(var_name.clone());
            } else if has_read_inside && has_write_before && has_write_inside {
                // Both read and written inside, also written before →
                // parameter (the initial value matters).
                result.parameters.push(var_name.clone());
                if has_read_after {
                    result.return_values.push(var_name.clone());
                }
            } else if has_write_inside && has_read_after {
                // Written inside, read after → return value.  It is ALSO a
                // parameter whenever the extracted code reads the variable's
                // *incoming* value.  That happens when it was written before
                // the range, or when a read inside the range occurs before
                // the first pure write inside it (a read that consumes the
                // value passed in).  A `+=`-style read-write also reads the
                // incoming value, so a read-write that precedes any pure
                // write counts too.
                let first_pure_write_inside = frame_accesses
                    .iter()
                    .filter(|a| {
                        a.offset >= start && a.offset < end && matches!(a.kind, AccessKind::Write)
                    })
                    .map(|a| a.offset)
                    .min();
                let reads_incoming_value = frame_accesses.iter().any(|a| {
                    a.offset >= start
                        && a.offset < end
                        && matches!(a.kind, AccessKind::Read | AccessKind::ReadWrite)
                        && first_pure_write_inside.is_none_or(|w| a.offset < w)
                });
                let needs_param = has_write_before || reads_incoming_value;
                if needs_param && !result.parameters.contains(var_name) {
                    result.parameters.push(var_name.clone());
                }
                if !result.return_values.contains(var_name) {
                    result.return_values.push(var_name.clone());
                }
            } else if has_read_inside && !has_write_before && !has_write_inside {
                // Read inside but never written — could be a global or
                // an undefined variable.  Treat as parameter.
                result.parameters.push(var_name.clone());
            } else if has_write_inside && !has_read_inside && !has_read_after {
                // Written inside but never read anywhere — local (dead write).
                result.locals.push(var_name.clone());
            }
        }

        // Sort for deterministic output.
        result.parameters.sort();
        result.return_values.sort();
        result.locals.sort();
        result.reference_writes.sort();

        result
    }

    /// Return all unique variable names accessed within the enclosing
    /// frame of the given offset.
    pub(crate) fn variables_in_scope(&self, offset: u32) -> Vec<String> {
        let frame = match self.enclosing_frame(offset) {
            Some(f) => f,
            None => return Vec::new(),
        };

        let mut names: Vec<String> = Vec::new();
        for access in &self.accesses {
            if access.offset >= frame.start
                && access.offset <= frame.end
                && !names.contains(&access.name)
                && access.name != "$this"
            {
                names.push(access.name.clone());
            }
        }
        names.sort();
        names
    }

    /// Return all offsets where variable `name` is accessed within the
    /// enclosing frame of the given offset.  Useful for document
    /// highlights / find-references within a scope.
    pub(crate) fn all_occurrences(&self, name: &str, offset: u32) -> Vec<(u32, AccessKind)> {
        let frame = match self.enclosing_frame(offset) {
            Some(f) => f,
            None => return Vec::new(),
        };

        self.accesses_in_frame(name, frame)
            .into_iter()
            .map(|a| (a.offset, a.kind))
            .collect()
    }
}

// ─── Collection (forward-pass AST walker) ───────────────────────────────────

/// Internal state for the forward-pass walker.
struct Collector<'a> {
    accesses: Vec<VarAccess>,
    frames: Vec<Frame>,
    has_this_or_self: bool,
    has_reference_params: bool,
    /// Stack of frame start offsets for determining the current scope.
    frame_stack: Vec<u32>,
    /// Optional callback that resolves by-reference parameter positions
    /// for function/static-method/constructor calls.
    by_ref_resolver: Option<ByRefResolver<'a>>,
    /// Name of the enclosing class (when walking a method body).
    /// Used to resolve `$this->method()` by-ref parameters.
    enclosing_class_name: Option<String>,
}

impl<'a> Collector<'a> {
    fn new() -> Self {
        Collector {
            accesses: Vec::new(),
            frames: Vec::new(),
            has_this_or_self: false,
            has_reference_params: false,
            frame_stack: Vec::new(),
            by_ref_resolver: None,
            enclosing_class_name: None,
        }
    }

    fn with_resolver(resolver: ByRefResolver<'a>) -> Self {
        Collector {
            accesses: Vec::new(),
            frames: Vec::new(),
            has_this_or_self: false,
            has_reference_params: false,
            frame_stack: Vec::new(),
            by_ref_resolver: Some(resolver),
            enclosing_class_name: None,
        }
    }

    /// Return the enclosing class name, if set.
    fn enclosing_class_name(&self) -> Option<&str> {
        self.enclosing_class_name.as_deref()
    }

    fn push_access(&mut self, name: String, offset: u32, kind: AccessKind) {
        self.accesses.push(VarAccess { name, offset, kind });
    }

    fn push_frame(&mut self, frame: Frame) {
        self.frame_stack.push(frame.start);
        self.frames.push(frame);
    }

    fn pop_frame(&mut self) {
        self.frame_stack.pop();
    }
}

/// Build a [`ScopeMap`] for the function/method/closure body that
/// contains `offset`.  Walks top-level and namespaced statements to
/// find the enclosing body, then collects variable accesses within it.
///
/// This is the shared implementation behind the `build_scope_map`
/// helpers in extract-function, extract-variable, and inline-variable
/// code actions.  All three need the same "find enclosing scope, then
/// collect" pattern.
pub(crate) fn build_scope_map_for_offset(
    statements: &[Statement<'_>],
    offset: u32,
    content_len: u32,
) -> ScopeMap {
    for stmt in statements {
        if let Some(map) = try_build_scope_from_statement(stmt, offset) {
            return map;
        }
    }
    // Fallback: top-level scope.
    collect_scope(statements, 0, content_len)
}

/// Recursively try to build a scope map from a single statement that
/// contains `offset`.
fn try_build_scope_from_statement(stmt: &Statement<'_>, offset: u32) -> Option<ScopeMap> {
    match stmt {
        Statement::Function(func) => {
            let body_start = func.body.left_brace.start.offset;
            let body_end = func.body.right_brace.end.offset;
            if offset >= body_start && offset <= body_end {
                return Some(collect_function_scope_with_kind(
                    &func.parameter_list,
                    func.body.statements.as_slice(),
                    body_start,
                    body_end,
                    FrameKind::Function,
                ));
            }
        }
        Statement::Class(class) => {
            for member in class.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
                    let body_start = block.left_brace.start.offset;
                    let body_end = block.right_brace.end.offset;
                    if offset >= body_start && offset <= body_end {
                        return Some(collect_function_scope_with_kind(
                            &method.parameter_list,
                            block.statements.as_slice(),
                            body_start,
                            body_end,
                            FrameKind::Method,
                        ));
                    }
                }
            }
        }
        Statement::Trait(tr) => {
            for member in tr.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
                    let body_start = block.left_brace.start.offset;
                    let body_end = block.right_brace.end.offset;
                    if offset >= body_start && offset <= body_end {
                        return Some(collect_function_scope_with_kind(
                            &method.parameter_list,
                            block.statements.as_slice(),
                            body_start,
                            body_end,
                            FrameKind::Method,
                        ));
                    }
                }
            }
        }
        Statement::Enum(en) => {
            for member in en.members.iter() {
                if let ClassLikeMember::Method(method) = member
                    && let MethodBody::Concrete(block) = &method.body
                {
                    let body_start = block.left_brace.start.offset;
                    let body_end = block.right_brace.end.offset;
                    if offset >= body_start && offset <= body_end {
                        return Some(collect_function_scope_with_kind(
                            &method.parameter_list,
                            block.statements.as_slice(),
                            body_start,
                            body_end,
                            FrameKind::Method,
                        ));
                    }
                }
            }
        }
        Statement::Namespace(ns) => {
            for inner in ns.statements().iter() {
                if let Some(map) = try_build_scope_from_statement(inner, offset) {
                    return Some(map);
                }
            }
        }
        _ => {}
    }
    None
}

/// Collect all variable reads and writes within a function/method body.
///
/// `body_start` and `body_end` are the byte offsets of the opening `{`
/// and closing `}` of the function body.  The returned [`ScopeMap`]
/// contains a single top-level frame plus any nested frames (closures,
/// arrow functions, catch blocks).
pub(crate) fn collect_scope(
    statements: &[Statement<'_>],
    body_start: u32,
    body_end: u32,
) -> ScopeMap {
    collect_scope_with_resolver(statements, body_start, body_end, None)
}

/// Like [`collect_scope`] but accepts an optional [`ByRefResolver`]
/// callback for detecting by-reference parameters in user-defined
/// function and static method calls.
pub(crate) fn collect_scope_with_resolver(
    statements: &[Statement<'_>],
    body_start: u32,
    body_end: u32,
    resolver: Option<ByRefResolver<'_>>,
) -> ScopeMap {
    let mut collector = match resolver {
        Some(r) => Collector::with_resolver(r),
        None => Collector::new(),
    };

    collector.push_frame(Frame {
        start: body_start,
        end: body_end,
        kind: FrameKind::TopLevel,
        captures: Vec::new(),
        parameters: Vec::new(),
    });

    for stmt in statements {
        walk_statement(stmt, &mut collector);
    }

    collector.pop_frame();

    collector.frames.sort_by_key(|f| f.start);

    ScopeMap {
        accesses: collector.accesses,
        frames: collector.frames,
        has_this_or_self: collector.has_this_or_self,
        has_reference_params: collector.has_reference_params,
    }
}

/// Collect scope information for a set of function parameters.
///
/// Records each parameter as a `Write` access at its offset.
pub(crate) fn collect_parameters(
    params: &FunctionLikeParameterList<'_>,
    collector_accesses: &mut Vec<VarAccess>,
    collector_has_reference: &mut bool,
) {
    for param in params.parameters.iter() {
        let name = bytes_to_str(param.variable.name).to_string();
        let offset = param.variable.span().start.offset;
        collector_accesses.push(VarAccess {
            name,
            offset,
            kind: AccessKind::Write,
        });
        if param.ampersand.is_some() {
            *collector_has_reference = true;
        }
        if let Some(ref default) = param.default_value {
            let mut tmp = Collector::new();
            walk_expression(default.value, &mut tmp);
            collector_accesses.extend(tmp.accesses);
        }
    }
}

/// Convenience: collect scope from a full method or function AST node.
///
/// Includes parameter declarations and the body.
pub(crate) fn collect_function_scope<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
    body_start: u32,
    body_end: u32,
) -> ScopeMap {
    collect_function_scope_with_kind(params, body, body_start, body_end, FrameKind::Function)
}

/// Like [`collect_function_scope`] but accepts an optional
/// [`ByRefResolver`] callback.
pub(crate) fn collect_function_scope_with_resolver<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
    body_start: u32,
    body_end: u32,
    resolver: Option<ByRefResolver<'_>>,
) -> ScopeMap {
    collect_function_scope_with_kind_and_resolver(
        params,
        body,
        body_start,
        body_end,
        FrameKind::Function,
        resolver,
        None,
    )
}

/// Like [`collect_function_scope`] but allows specifying the
/// [`FrameKind`] for the outermost frame.  Use `FrameKind::Method`
/// when collecting inside a class method.
pub(crate) fn collect_function_scope_with_kind<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
    body_start: u32,
    body_end: u32,
    kind: FrameKind,
) -> ScopeMap {
    collect_function_scope_with_kind_and_resolver(
        params, body, body_start, body_end, kind, None, None,
    )
}

/// Like [`collect_function_scope_with_kind`] but accepts an optional
/// [`ByRefResolver`] callback for detecting by-reference parameters
/// in user-defined function and static method calls.
pub(crate) fn collect_function_scope_with_kind_and_resolver<'a>(
    params: &FunctionLikeParameterList<'a>,
    body: &[Statement<'a>],
    body_start: u32,
    body_end: u32,
    kind: FrameKind,
    resolver: Option<ByRefResolver<'_>>,
    enclosing_class_name: Option<String>,
) -> ScopeMap {
    let mut collector = match resolver {
        Some(r) => Collector::with_resolver(r),
        None => Collector::new(),
    };
    collector.enclosing_class_name = enclosing_class_name;

    let param_names: Vec<String> = params
        .parameters
        .iter()
        .map(|p| bytes_to_str(p.variable.name).to_string())
        .collect();

    collector.push_frame(Frame {
        start: body_start,
        end: body_end,
        kind,
        captures: Vec::new(),
        parameters: param_names,
    });

    // Record parameters as writes.
    collect_parameters(
        params,
        &mut collector.accesses,
        &mut collector.has_reference_params,
    );

    for stmt in body {
        walk_statement(stmt, &mut collector);
    }

    collector.pop_frame();

    collector.frames.sort_by_key(|f| f.start);

    ScopeMap {
        accesses: collector.accesses,
        frames: collector.frames,
        has_this_or_self: collector.has_this_or_self,
        has_reference_params: collector.has_reference_params,
    }
}

// ─── Statement walker ───────────────────────────────────────────────────────

fn walk_statement(stmt: &Statement<'_>, collector: &mut Collector<'_>) {
    match stmt {
        Statement::Expression(expr_stmt) => {
            walk_expression(expr_stmt.expression, collector);
        }
        Statement::Return(ret) => {
            if let Some(val) = ret.value {
                walk_expression(val, collector);
            }
        }
        Statement::Echo(echo) => {
            for val in echo.values.iter() {
                walk_expression(val, collector);
            }
        }
        Statement::If(if_stmt) => {
            walk_expression(if_stmt.condition, collector);
            match &if_stmt.body {
                IfBody::Statement(if_body) => {
                    walk_if_statement_body(if_body, collector);
                }
                IfBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        walk_statement(s, collector);
                    }
                    for clause in body.else_if_clauses.iter() {
                        walk_expression(clause.condition, collector);
                        for s in clause.statements.iter() {
                            walk_statement(s, collector);
                        }
                    }
                    if let Some(ref else_clause) = body.else_clause {
                        for s in else_clause.statements.iter() {
                            walk_statement(s, collector);
                        }
                    }
                }
            }
        }
        Statement::Foreach(foreach) => {
            walk_expression(foreach.expression, collector);
            // The key and value bindings are writes.
            if let Some(key_expr) = foreach.target.key() {
                walk_expression_as_write(key_expr, collector);
            }
            let value_expr = foreach.target.value();
            if value_expr.is_reference() {
                // By-reference foreach (`&$value`): the binding is
                // semantically "used" because writes through the
                // reference mutate the original array.  Record as
                // ReadWrite so the unused-variable diagnostic does
                // not fire on the foreach variable.
                walk_expression_as_readwrite(value_expr, collector);
            } else {
                walk_expression_as_write(value_expr, collector);
            }

            match &foreach.body {
                ForeachBody::Statement(inner) => {
                    walk_statement(inner, collector);
                }
                ForeachBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        walk_statement(s, collector);
                    }
                }
            }
        }
        Statement::While(while_stmt) => {
            walk_expression(while_stmt.condition, collector);
            match &while_stmt.body {
                WhileBody::Statement(inner) => {
                    walk_statement(inner, collector);
                }
                WhileBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        walk_statement(s, collector);
                    }
                }
            }
        }
        Statement::DoWhile(dw) => {
            walk_statement(dw.statement, collector);
            walk_expression(dw.condition, collector);
        }
        Statement::For(for_stmt) => {
            for init in for_stmt.initializations.iter() {
                walk_expression(init, collector);
            }
            for cond in for_stmt.conditions.iter() {
                walk_expression(cond, collector);
            }
            for inc in for_stmt.increments.iter() {
                walk_expression(inc, collector);
            }
            match &for_stmt.body {
                ForBody::Statement(inner) => {
                    walk_statement(inner, collector);
                }
                ForBody::ColonDelimited(body) => {
                    for s in body.statements.iter() {
                        walk_statement(s, collector);
                    }
                }
            }
        }
        Statement::Switch(switch) => {
            walk_expression(switch.expression, collector);
            for case in switch.body.cases().iter() {
                match case {
                    SwitchCase::Expression(c) => {
                        walk_expression(c.expression, collector);
                        for s in c.statements.iter() {
                            walk_statement(s, collector);
                        }
                    }
                    SwitchCase::Default(c) => {
                        for s in c.statements.iter() {
                            walk_statement(s, collector);
                        }
                    }
                }
            }
        }
        Statement::Try(try_stmt) => {
            for s in try_stmt.block.statements.iter() {
                walk_statement(s, collector);
            }
            for catch in try_stmt.catch_clauses.iter() {
                let catch_start = catch.block.left_brace.start.offset;
                let catch_end = catch.block.right_brace.end.offset;
                let catch_params = if let Some(ref var) = catch.variable {
                    vec![bytes_to_str(var.name).to_string()]
                } else {
                    Vec::new()
                };
                collector.push_frame(Frame {
                    start: catch_start,
                    end: catch_end,
                    kind: FrameKind::Catch,
                    captures: Vec::new(),
                    parameters: catch_params,
                });
                if let Some(ref var) = catch.variable {
                    let name = bytes_to_str(var.name).to_string();
                    collector.push_access(name, var.span().start.offset, AccessKind::Write);
                }
                for s in catch.block.statements.iter() {
                    walk_statement(s, collector);
                }
                collector.pop_frame();
            }
            if let Some(ref finally) = try_stmt.finally_clause {
                for s in finally.block.statements.iter() {
                    walk_statement(s, collector);
                }
            }
        }
        Statement::Block(block) => {
            for s in block.statements.iter() {
                walk_statement(s, collector);
            }
        }
        Statement::Unset(unset) => {
            for val in unset.values.iter() {
                // Unset is conceptually a write (destroying the variable).
                walk_expression_as_write(val, collector);
            }
        }
        Statement::Global(global) => {
            for var in global.variables.iter() {
                if let Variable::Direct(dv) = var {
                    let name = bytes_to_str(dv.name).to_string();
                    collector.push_access(name, dv.span().start.offset, AccessKind::Write);
                }
            }
        }
        Statement::Static(static_stmt) => {
            for item in static_stmt.items.iter() {
                let dv = item.variable();
                let name = bytes_to_str(dv.name).to_string();
                collector.push_access(name, dv.span().start.offset, AccessKind::Write);
                // Note: we don't walk the default value expression of
                // static items — it's evaluated once at first call and
                // rarely contains variable references.
            }
        }
        Statement::Namespace(ns) => {
            for s in ns.statements().iter() {
                walk_statement(s, collector);
            }
        }
        // Skip class/interface/trait/enum/function declarations — they have
        // their own scopes and don't leak variables.
        Statement::Class(_)
        | Statement::Interface(_)
        | Statement::Trait(_)
        | Statement::Enum(_)
        | Statement::Function(_) => {}

        // Labels / gotos / breaks / continues / declares / nops / use —
        // no variable accesses.
        _ => {}
    }
}

fn walk_if_statement_body(if_body: &IfStatementBody<'_>, collector: &mut Collector<'_>) {
    walk_statement(if_body.statement, collector);
    for clause in if_body.else_if_clauses.iter() {
        walk_expression(clause.condition, collector);
        walk_statement(clause.statement, collector);
    }
    if let Some(ref else_clause) = if_body.else_clause {
        walk_statement(else_clause.statement, collector);
    }
}

// ─── Expression walker ──────────────────────────────────────────────────────

fn walk_expression(expr: &Expression<'_>, collector: &mut Collector<'_>) {
    match expr {
        Expression::Variable(var) => {
            walk_variable_read(var, collector);
        }
        Expression::Assignment(assignment) => {
            walk_assignment(assignment, collector);
        }
        // ── Function / method / static-method calls ──
        Expression::Call(call) => match call {
            Call::Function(func_call) => {
                walk_expression(func_call.function, collector);
                walk_function_call_arguments(func_call, collector);
            }
            Call::Method(method_call) => {
                walk_expression(method_call.object, collector);
                walk_member_selector_read(&method_call.method, collector);
                walk_method_call_arguments_inner(
                    method_call.object,
                    &method_call.method,
                    &method_call.argument_list,
                    collector,
                );
            }
            Call::NullSafeMethod(method_call) => {
                walk_expression(method_call.object, collector);
                walk_member_selector_read(&method_call.method, collector);
                walk_method_call_arguments_inner(
                    method_call.object,
                    &method_call.method,
                    &method_call.argument_list,
                    collector,
                );
            }
            Call::StaticMethod(static_call) => {
                walk_expression(static_call.class, collector);
                walk_member_selector_read(&static_call.method, collector);
                walk_static_method_call_arguments(static_call, collector);
            }
        },
        // ── Property / constant access ──
        Expression::Access(access) => match access {
            Access::Property(pa) => {
                walk_expression(pa.object, collector);
                walk_member_selector_read(&pa.property, collector);
            }
            Access::NullSafeProperty(pa) => {
                walk_expression(pa.object, collector);
                walk_member_selector_read(&pa.property, collector);
            }
            Access::StaticProperty(spa) => {
                walk_expression(spa.class, collector);
                // Direct is a member name (self::$prop), not a local variable.
                // Indirect/Nested contain expressions that may reference local vars
                // (e.g. self::$$prop, self::${expr}).
                match &spa.property {
                    Variable::Direct(_) => {}
                    Variable::Indirect(iv) => walk_expression(iv.expression, collector),
                    Variable::Nested(nv) => walk_variable_read(nv.variable, collector),
                }
            }
            Access::ClassConstant(cca) => {
                walk_expression(cca.class, collector);
            }
        },
        Expression::ArrayAccess(access) => {
            walk_expression(access.array, collector);
            walk_expression(access.index, collector);
        }
        Expression::ArrayAppend(append) => {
            walk_expression(append.array, collector);
        }
        Expression::Array(array) => {
            for element in array.elements.iter() {
                walk_array_element(element, collector);
            }
        }
        Expression::LegacyArray(array) => {
            for element in array.elements.iter() {
                walk_array_element(element, collector);
            }
        }
        Expression::List(list) => {
            // In read position, list entries are reads.
            for element in list.elements.iter() {
                walk_array_element(element, collector);
            }
        }
        Expression::Closure(closure) => {
            walk_closure(closure, collector);
        }
        Expression::ArrowFunction(arrow) => {
            walk_arrow_function(arrow, collector);
        }
        Expression::Parenthesized(paren) => {
            walk_expression(paren.expression, collector);
        }
        Expression::UnaryPrefix(unary) => {
            walk_expression(unary.operand, collector);
        }
        Expression::UnaryPostfix(unary) => {
            // `$x++` and `$x--` are read-write.
            if let Expression::Variable(Variable::Direct(dv)) = unary.operand {
                let name = bytes_to_str(dv.name).to_string();
                collector.push_access(name, dv.span().start.offset, AccessKind::ReadWrite);
            } else {
                walk_expression(unary.operand, collector);
            }
        }
        Expression::Binary(binary) => {
            walk_expression(binary.lhs, collector);
            walk_expression(binary.rhs, collector);
        }
        // ── Ternary / short ternary / null coalescing ──
        Expression::Conditional(cond) => {
            walk_expression(cond.condition, collector);
            if let Some(then_expr) = cond.then {
                walk_expression(then_expr, collector);
            }
            walk_expression(cond.r#else, collector);
        }
        Expression::Instantiation(inst) => {
            walk_expression(inst.class, collector);
            if let Some(ref args) = inst.argument_list {
                walk_constructor_arguments(inst, args, collector);
            }
        }
        Expression::Throw(throw) => {
            walk_expression(throw.exception, collector);
        }
        Expression::Yield(yield_expr) => match yield_expr {
            Yield::Value(yv) => {
                if let Some(val) = yv.value {
                    walk_expression(val, collector);
                }
            }
            Yield::Pair(yp) => {
                walk_expression(yp.key, collector);
                walk_expression(yp.value, collector);
            }
            Yield::From(yf) => {
                walk_expression(yf.iterator, collector);
            }
        },
        Expression::Clone(clone) => {
            walk_expression(clone.object, collector);
        }
        Expression::Match(match_expr) => {
            walk_expression(match_expr.expression, collector);
            for arm in match_expr.arms.iter() {
                match arm {
                    MatchArm::Expression(expr_arm) => {
                        for cond in expr_arm.conditions.iter() {
                            walk_expression(cond, collector);
                        }
                        walk_expression(expr_arm.expression, collector);
                    }
                    MatchArm::Default(default_arm) => {
                        walk_expression(default_arm.expression, collector);
                    }
                }
            }
        }
        // ── self / static / parent keywords ──
        Expression::Self_(_) => {
            collector.has_this_or_self = true;
            collector.push_access(
                "self".to_string(),
                expr.span().start.offset,
                AccessKind::Read,
            );
        }
        Expression::Static(_) => {
            collector.has_this_or_self = true;
            collector.push_access(
                "static".to_string(),
                expr.span().start.offset,
                AccessKind::Read,
            );
        }
        Expression::Parent(_) => {
            collector.has_this_or_self = true;
            collector.push_access(
                "parent".to_string(),
                expr.span().start.offset,
                AccessKind::Read,
            );
        }
        // ── Language constructs ──
        Expression::Construct(construct) => match construct {
            Construct::Isset(isset) => {
                for val in isset.values.iter() {
                    walk_expression(val, collector);
                }
            }
            Construct::Empty(empty) => {
                walk_expression(empty.value, collector);
            }
            Construct::Eval(eval) => {
                walk_expression(eval.value, collector);
            }
            Construct::Include(inc) => {
                walk_expression(inc.value, collector);
            }
            Construct::IncludeOnce(inc) => {
                walk_expression(inc.value, collector);
            }
            Construct::Require(req) => {
                walk_expression(req.value, collector);
            }
            Construct::RequireOnce(req) => {
                walk_expression(req.value, collector);
            }
            Construct::Print(print) => {
                walk_expression(print.value, collector);
            }
            Construct::Exit(exit) => {
                if let Some(ref args) = exit.arguments {
                    walk_arguments(args, collector);
                }
            }
            Construct::Die(die) => {
                if let Some(ref args) = die.arguments {
                    walk_arguments(args, collector);
                }
            }
        },
        // ── Composite strings (interpolation, heredoc, shell-exec) ──
        Expression::CompositeString(composite) => {
            for part in composite.parts().iter() {
                match part {
                    StringPart::Expression(inner_expr) => {
                        walk_expression(inner_expr, collector);
                    }
                    StringPart::BracedExpression(braced) => {
                        walk_expression(braced.expression, collector);
                    }
                    StringPart::Literal(_) => {}
                }
            }
        }
        // ── Constant access ──
        Expression::ConstantAccess(_) => {
            // No variable accesses in standalone constant references.
        }
        // ── Pipe operator (PHP 8.5) ──
        Expression::Pipe(pipe) => {
            walk_expression(pipe.input, collector);
            walk_expression(pipe.callable, collector);
        }
        // ── First-class callable / partial application ──
        Expression::PartialApplication(partial) => match partial {
            PartialApplication::Function(func_pa) => {
                walk_expression(func_pa.function, collector);
            }
            PartialApplication::Method(method_pa) => {
                walk_expression(method_pa.object, collector);
            }
            PartialApplication::StaticMethod(static_pa) => {
                walk_expression(static_pa.class, collector);
            }
        },
        // ── Anonymous class ──
        Expression::AnonymousClass(anon) => {
            if let Some(ref args) = anon.argument_list {
                walk_partial_arguments(args, collector);
            }
            // Skip members — anonymous class body is a separate scope.
        }

        // Non-navigable expressions (literals, identifiers, magic constants, etc.)
        Expression::Literal(_)
        | Expression::MagicConstant(_)
        | Expression::Identifier(_)
        | Expression::Error(_) => {}

        // Catch-all for any remaining expression types.
        _ => {}
    }
}

/// Walk an expression that appears in a write position (LHS of
/// assignment, foreach binding, unset argument, etc.).
///
/// See also [`walk_expression_as_readwrite`] for by-reference foreach bindings.
fn walk_expression_as_write(expr: &Expression<'_>, collector: &mut Collector<'_>) {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            let name = bytes_to_str(dv.name).to_string();
            collector.push_access(name, dv.span().start.offset, AccessKind::Write);
        }
        Expression::Variable(Variable::Indirect(iv)) => {
            walk_expression(iv.expression, collector);
        }
        Expression::Variable(Variable::Nested(nv)) => {
            walk_variable_read(nv.variable, collector);
        }
        Expression::Array(array) => {
            // Array destructuring: `[$a, $b] = …`
            for element in array.elements.iter() {
                match element {
                    ArrayElement::KeyValue(kv) => {
                        walk_expression_as_write(kv.value, collector);
                    }
                    ArrayElement::Value(v) => {
                        walk_expression_as_write(v.value, collector);
                    }
                    ArrayElement::Variadic(spread) => {
                        walk_expression_as_write(spread.value, collector);
                    }
                    ArrayElement::Missing(_) => {}
                }
            }
        }
        Expression::List(list) => {
            // list() destructuring: `list($a, $b) = …`
            for entry in list.elements.iter() {
                match entry {
                    ArrayElement::KeyValue(kv) => {
                        walk_expression_as_write(kv.value, collector);
                    }
                    ArrayElement::Value(v) => {
                        walk_expression_as_write(v.value, collector);
                    }
                    ArrayElement::Variadic(spread) => {
                        walk_expression_as_write(spread.value, collector);
                    }
                    ArrayElement::Missing(_) => {}
                }
            }
        }
        Expression::ArrayAccess(access) => {
            // `$arr[0] = …` — the array itself is being read-written.
            if let Expression::Variable(Variable::Direct(dv)) = access.array {
                let name = bytes_to_str(dv.name).to_string();
                collector.push_access(name, dv.span().start.offset, AccessKind::ReadWrite);
            } else {
                walk_expression_as_write(access.array, collector);
            }
            walk_expression(access.index, collector);
        }
        Expression::ArrayAppend(append) => {
            // `$arr[] = …` — the array itself is being read-written.
            if let Expression::Variable(Variable::Direct(dv)) = append.array {
                let name = bytes_to_str(dv.name).to_string();
                collector.push_access(name, dv.span().start.offset, AccessKind::ReadWrite);
            } else {
                walk_expression_as_write(append.array, collector);
            }
        }
        Expression::Access(Access::Property(pa)) => {
            // `$obj->prop = …` — $obj is read, prop is written.
            walk_expression(pa.object, collector);
            walk_member_selector_read(&pa.property, collector);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            walk_expression(pa.object, collector);
            walk_member_selector_read(&pa.property, collector);
        }
        Expression::Access(Access::StaticProperty(spa)) => {
            walk_expression(spa.class, collector);
            match &spa.property {
                Variable::Direct(_) => {}
                Variable::Indirect(iv) => walk_expression(iv.expression, collector),
                Variable::Nested(nv) => walk_variable_read(nv.variable, collector),
            }
        }
        Expression::UnaryPrefix(up) if matches!(up.operator, UnaryPrefixOperator::Reference(_)) => {
            // `&$value` in foreach — the inner variable is being written.
            walk_expression_as_write(up.operand, collector);
        }
        _ => {
            // For anything else, walk as read.
            walk_expression(expr, collector);
        }
    }
}

/// Walk an expression that is being both read and written (e.g. a
/// by-reference foreach binding like `&$value`).
fn walk_expression_as_readwrite(expr: &Expression<'_>, collector: &mut Collector<'_>) {
    match expr {
        Expression::UnaryPrefix(up) if matches!(up.operator, UnaryPrefixOperator::Reference(_)) => {
            walk_expression_as_readwrite(up.operand, collector);
        }
        Expression::Variable(Variable::Direct(dv)) => {
            let name = bytes_to_str(dv.name).to_string();
            collector.push_access(name, dv.span().start.offset, AccessKind::ReadWrite);
        }
        _ => {
            // Fallback: treat as a plain write.
            walk_expression_as_write(expr, collector);
        }
    }
}

/// Walk a variable in read position.
fn walk_variable_read(var: &Variable<'_>, collector: &mut Collector<'_>) {
    match var {
        Variable::Direct(dv) => {
            let name = bytes_to_str(dv.name).to_string();
            let offset = dv.span().start.offset;
            if name == "$this" {
                collector.has_this_or_self = true;
            }
            collector.push_access(name, offset, AccessKind::Read);
        }
        Variable::Indirect(iv) => {
            walk_expression(iv.expression, collector);
        }
        Variable::Nested(nv) => {
            walk_variable_read(nv.variable, collector);
        }
    }
}

/// Walk an assignment expression.
fn walk_assignment(assignment: &Assignment<'_>, collector: &mut Collector<'_>) {
    // Determine if this is a compound assignment (`+=`, `.=`, etc.)
    let is_compound = !assignment.operator.is_assign();

    if is_compound {
        // Compound assignment: LHS is both read and written.
        if let Expression::Variable(Variable::Direct(dv)) = assignment.lhs {
            let name = bytes_to_str(dv.name).to_string();
            collector.push_access(name, dv.span().start.offset, AccessKind::ReadWrite);
        } else {
            walk_expression(assignment.lhs, collector);
        }
    } else {
        // Simple assignment: LHS is written.
        walk_expression_as_write(assignment.lhs, collector);
    }

    // RHS is always read.
    walk_expression(assignment.rhs, collector);
}

/// Walk arguments in a function/method call.
fn walk_arguments(args: &ArgumentList<'_>, collector: &mut Collector<'_>) {
    for arg in args.arguments.iter() {
        walk_expression(arg.value(), collector);
    }
}

/// Walk partial arguments in a function/method call.
fn walk_partial_arguments(args: &PartialArgumentList<'_>, collector: &mut Collector<'_>) {
    for arg in args.arguments.iter() {
        if let Some(value) = arg.value() {
            walk_expression(value, collector);
        }
    }
}

/// Known PHP functions with by-reference out-parameters.
///
/// Each entry maps a function name to a list of 0-based argument
/// positions that are by-reference out-parameters.  When the scope
/// collector encounters a call to one of these functions, those
/// argument positions are recorded as writes instead of reads.
const BY_REF_OUT_PARAMS: &[(&str, &[usize])] = &[
    // Regex
    ("preg_match", &[2]),
    ("preg_match_all", &[2]),
    ("preg_replace", &[4]),
    ("preg_replace_callback", &[4]),
    // String/parsing
    ("exec", &[1, 2]),
    ("mb_parse_str", &[1]),
    ("parse_str", &[1]),
    ("similar_text", &[2]),
    ("sscanf", &[2, 3, 4, 5, 6, 7]),
    // cURL
    ("curl_multi_exec", &[1]),
    ("curl_multi_info_read", &[1]),
    // Sockets/streams
    ("fsockopen", &[2, 3]),
    ("pfsockopen", &[2, 3]),
    ("socket_create_pair", &[3]),
    ("stream_socket_accept", &[2]),
    ("stream_socket_client", &[1, 2]),
    ("stream_socket_recvfrom", &[3]),
    ("stream_socket_server", &[1, 2]),
    // OpenSSL
    ("openssl_csr_new", &[1]),
    ("openssl_encrypt", &[6]),
    ("openssl_open", &[1]),
    ("openssl_pkey_export", &[1]),
    ("openssl_pkcs12_export", &[1]),
    ("openssl_pkcs12_read", &[1]),
    ("openssl_public_encrypt", &[1]),
    ("openssl_random_pseudo_bytes", &[1]),
    ("openssl_seal", &[1, 2, 6]),
    ("openssl_sign", &[1]),
    // Process
    ("pcntl_wait", &[0]),
    ("pcntl_waitpid", &[1]),
    // Image
    ("exif_thumbnail", &[1, 2, 3]),
    ("getimagesize", &[1]),
    ("getimagesizefromstring", &[1]),
    // DNS
    ("dns_get_mx", &[1, 2]),
    ("dns_get_record", &[2, 3]),
    ("getmxrr", &[1, 2]),
    // File/IO
    ("flock", &[2]),
    // IPC
    ("msg_receive", &[2, 4, 7]),
    ("msg_send", &[5]),
    // LDAP
    ("ldap_parse_result", &[2, 3, 4, 5, 6]),
    // Misc
    ("getopt", &[2]),
    ("grapheme_extract", &[4]),
    ("headers_sent", &[0, 1]),
];

/// Walk arguments for a direct function call, treating known
/// by-reference out-parameters as writes.
fn walk_function_call_arguments(func_call: &FunctionCall<'_>, collector: &mut Collector<'_>) {
    // Try to extract a bare function name.
    let func_name = match func_call.function {
        Expression::Identifier(ident) => {
            let raw = bytes_to_str(ident.value());
            // Strip leading backslash for FQN calls like \preg_match
            raw.strip_prefix('\\').unwrap_or(raw)
        }
        _ => {
            // Not a bare name — fall through to default handling.
            walk_arguments(&func_call.argument_list, collector);
            return;
        }
    };

    // Look up by-ref out-parameter positions: first the hardcoded
    // table, then the optional resolver callback.
    let hardcoded = BY_REF_OUT_PARAMS
        .iter()
        .find(|(name, _)| *name == func_name)
        .map(|(_, positions)| *positions);

    let resolved_positions: Vec<usize>;
    let out_positions: Option<&[usize]> = if let Some(positions) = hardcoded {
        Some(positions)
    } else if let Some(ref resolver) = collector.by_ref_resolver {
        let kind = ByRefCallKind::Function(func_name);
        if let Some(positions) = resolver(&kind) {
            resolved_positions = positions;
            if resolved_positions.is_empty() {
                None
            } else {
                Some(&resolved_positions)
            }
        } else {
            None
        }
    } else {
        None
    };

    match out_positions {
        Some(positions) => {
            for (idx, arg) in func_call.argument_list.arguments.iter().enumerate() {
                if positions.contains(&idx) {
                    // By-ref out-parameters are recorded as ReadWrite:
                    // the variable is "used" by being passed to the
                    // function, even though the function overwrites it.
                    // This prevents false "unused variable" diagnostics
                    // for variables that exist solely to satisfy PHP's API
                    // (e.g. `getmxrr($domain, $dummy)`).
                    walk_expression_as_readwrite(arg.value(), collector);
                } else {
                    walk_expression(arg.value(), collector);
                }
            }
        }
        None => {
            walk_arguments(&func_call.argument_list, collector);
        }
    }
}

/// Walk a member-name selector (the `method` in `$obj->{$name}()` or the
/// property in `$obj->{$name}`) as a read position.  A dynamic selector
/// references a local variable that must count as used: `Variable`
/// (`$obj->$name()`) and braced `Expression` (`$obj->{$name}()`) forms
/// both read whatever variable they contain.  Static `Identifier` and
/// `Missing` selectors contain no variable reads.
fn walk_member_selector_read(
    selector: &ClassLikeMemberSelector<'_>,
    collector: &mut Collector<'_>,
) {
    match selector {
        ClassLikeMemberSelector::Identifier(_) | ClassLikeMemberSelector::Missing(_) => {}
        ClassLikeMemberSelector::Variable(var) => walk_variable_read(var, collector),
        ClassLikeMemberSelector::Expression(selector) => {
            walk_expression(selector.expression, collector)
        }
    }
}

/// Walk arguments for an instance method call (`$obj->method(...)`),
/// checking the optional resolver for by-reference parameter positions
/// when the receiver is `$this`.
fn walk_method_call_arguments_inner(
    object: &Expression<'_>,
    method: &ClassLikeMemberSelector<'_>,
    argument_list: &ArgumentList<'_>,
    collector: &mut Collector<'_>,
) {
    // Only handle `$this->method()` — for arbitrary receivers we cannot
    // resolve the class type in the scope collector.
    let is_this =
        matches!(object, Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this");
    if !is_this {
        walk_arguments(argument_list, collector);
        return;
    }

    let method_name = match method {
        ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
        _ => {
            walk_arguments(argument_list, collector);
            return;
        }
    };

    // Find the enclosing class name from the collector's class stack.
    let enclosing_class = collector.enclosing_class_name();
    let enclosing_class = match enclosing_class {
        Some(name) => name,
        None => {
            walk_arguments(argument_list, collector);
            return;
        }
    };

    if let Some(ref resolver) = collector.by_ref_resolver {
        let kind = ByRefCallKind::InstanceMethod(enclosing_class, method_name);
        if let Some(positions) = resolver(&kind)
            && !positions.is_empty()
        {
            for (idx, arg) in argument_list.arguments.iter().enumerate() {
                if positions.contains(&idx) {
                    walk_expression_as_readwrite(arg.value(), collector);
                } else {
                    walk_expression(arg.value(), collector);
                }
            }
            return;
        }
    }

    walk_arguments(argument_list, collector);
}

fn walk_static_method_call_arguments(
    static_call: &StaticMethodCall<'_>,
    collector: &mut Collector<'_>,
) {
    // Extract the class name from the AST.
    let class_name = match static_call.class {
        Expression::Identifier(ident) => bytes_to_str(ident.value()),
        Expression::Self_(_) => "self",
        Expression::Static(_) => "static",
        Expression::Parent(_) => "parent",
        _ => {
            walk_arguments(&static_call.argument_list, collector);
            return;
        }
    };

    let method_name = match &static_call.method {
        ClassLikeMemberSelector::Identifier(ident) => bytes_to_str(ident.value),
        // Variable method name (`Cls::$method()`) — can't resolve statically.
        _ => {
            walk_arguments(&static_call.argument_list, collector);
            return;
        }
    };

    if let Some(ref resolver) = collector.by_ref_resolver {
        let kind = ByRefCallKind::StaticMethod(class_name, method_name);
        if let Some(positions) = resolver(&kind)
            && !positions.is_empty()
        {
            for (idx, arg) in static_call.argument_list.arguments.iter().enumerate() {
                if positions.contains(&idx) {
                    walk_expression_as_readwrite(arg.value(), collector);
                } else {
                    walk_expression(arg.value(), collector);
                }
            }
            return;
        }
    }

    walk_arguments(&static_call.argument_list, collector);
}

/// Walk arguments for a constructor call (`new Cls(...)`), checking
/// the optional resolver for by-reference parameter positions.
fn walk_constructor_arguments(
    inst: &Instantiation<'_>,
    args: &ArgumentList<'_>,
    collector: &mut Collector<'_>,
) {
    // Extract the class name from the AST.
    let class_name = match inst.class {
        Expression::Identifier(ident) => bytes_to_str(ident.value()),
        Expression::Self_(_) => "self",
        Expression::Static(_) => "static",
        Expression::Parent(_) => "parent",
        _ => {
            walk_arguments(args, collector);
            return;
        }
    };

    if let Some(ref resolver) = collector.by_ref_resolver {
        let kind = ByRefCallKind::Constructor(class_name);
        if let Some(positions) = resolver(&kind)
            && !positions.is_empty()
        {
            for (idx, arg) in args.arguments.iter().enumerate() {
                if positions.contains(&idx) {
                    walk_expression_as_readwrite(arg.value(), collector);
                } else {
                    walk_expression(arg.value(), collector);
                }
            }
            return;
        }
    }

    walk_arguments(args, collector);
}

/// Walk array elements in a read context.
fn walk_array_element(element: &ArrayElement<'_>, collector: &mut Collector<'_>) {
    match element {
        ArrayElement::KeyValue(kv) => {
            walk_expression(kv.key, collector);
            walk_expression(kv.value, collector);
        }
        ArrayElement::Value(v) => {
            walk_expression(v.value, collector);
        }
        ArrayElement::Variadic(spread) => {
            walk_expression(spread.value, collector);
        }
        ArrayElement::Missing(_) => {}
    }
}

/// Walk a closure expression.
///
/// Creates a new frame for the closure body.  Records `use()` captures
/// and parameter declarations as writes in the new frame.
fn walk_closure(closure: &Closure<'_>, collector: &mut Collector<'_>) {
    let body_start = closure.body.left_brace.start.offset;
    let body_end = closure.body.right_brace.end.offset;

    let mut captures = Vec::new();
    if let Some(ref use_clause) = closure.use_clause {
        for var in use_clause.variables.iter() {
            let name = bytes_to_str(var.variable.name).to_string();
            let is_ref = var.ampersand.is_some();
            captures.push((name.clone(), is_ref));

            // The captured variable is a read in the outer scope at
            // the `use(...)` site.
            collector.push_access(name, var.variable.span().start.offset, AccessKind::Read);
        }
    }

    let param_names: Vec<String> = closure
        .parameter_list
        .parameters
        .iter()
        .map(|p| bytes_to_str(p.variable.name).to_string())
        .collect();

    collector.push_frame(Frame {
        start: body_start,
        end: body_end,
        kind: FrameKind::Closure,
        captures: captures.clone(),
        parameters: param_names,
    });

    // Record parameters as writes in the closure frame.
    for param in closure.parameter_list.parameters.iter() {
        let name = bytes_to_str(param.variable.name).to_string();
        let offset = param.variable.span().start.offset;
        collector.push_access(name, offset, AccessKind::Write);
        if param.ampersand.is_some() {
            collector.has_reference_params = true;
        }
    }

    // Record captures as writes in the closure frame.
    for (cap_name, _is_ref) in &captures {
        collector.push_access(cap_name.clone(), body_start, AccessKind::Write);
    }

    for stmt in closure.body.statements.iter() {
        walk_statement(stmt, collector);
    }

    collector.pop_frame();
}

/// Walk an arrow function expression.
///
/// Creates a new frame for the arrow function body expression.  Arrow
/// functions implicitly capture all outer variables by value.
fn walk_arrow_function(arrow: &ArrowFunction<'_>, collector: &mut Collector<'_>) {
    let body_start = arrow.arrow.start.offset;
    let body_end = arrow.expression.span().end.offset;

    let param_names: Vec<String> = arrow
        .parameter_list
        .parameters
        .iter()
        .map(|p| bytes_to_str(p.variable.name).to_string())
        .collect();

    collector.push_frame(Frame {
        start: body_start,
        end: body_end,
        kind: FrameKind::ArrowFunction,
        captures: Vec::new(), // Arrow functions capture implicitly.
        parameters: param_names,
    });

    // Record parameters as writes.
    for param in arrow.parameter_list.parameters.iter() {
        let name = bytes_to_str(param.variable.name).to_string();
        let offset = param.variable.span().start.offset;
        collector.push_access(name, offset, AccessKind::Write);
        if param.ampersand.is_some() {
            collector.has_reference_params = true;
        }
    }

    walk_expression(arrow.expression, collector);

    collector.pop_frame();
}
