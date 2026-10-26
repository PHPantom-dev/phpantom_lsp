//! The context a forward walk carries: loaders, the enclosing class and
//! function, the source text, the per-walk settings every statement and
//! expression handler reads, and the walk's control-flow state.

use super::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;

use crate::php_type::PhpType;
use crate::type_engine::resolver::{
    FileCtx, Loaders, ResolutionCtx, VarResolutionCtx, WrittenNames,
};
use crate::types::{ClassInfo, ResolvedType};

/// The control-flow state of one forward walk.
///
/// Every copy of a walk's [`ForwardWalkCtx`] borrows the same one, so the
/// loops, `switch`es and closures the walk is inside of are tracked across
/// the copies it makes for a loop body or a closure.  A walk over a body
/// of its own (a callee read for its return type, a variable query) owns a
/// fresh one, so nothing the walk around it is doing changes its answer.
#[derive(Default)]
pub(crate) struct WalkFlow {
    /// The loop nesting depth (foreach, while, for, do-while).  Used to
    /// reduce the number of loop iterations for deeply nested loops,
    /// preventing the exponential blowup that occurs when loop iteration
    /// interacts with if-branch merging.
    pub(super) loop_depth: Cell<u32>,
    /// One frame per enclosing breakable structure (loop or `switch`),
    /// innermost last.  A `break`/`continue` writes the scope it leaves
    /// with into the frame its level names, and the structure that owns
    /// the frame folds those states into its own join.
    pub(super) exit_edges: RefCell<Vec<ExitEdges>>,
    /// One frame per closure body being walked for the states its
    /// `return`s leave with, innermost last.
    pub(super) return_edges: RefCell<Vec<ReturnFrame>>,
    /// Byte ranges of the branches a decidable guard rules out, when this
    /// walk collects them (the diagnostic pass's walk of a file).
    pub(super) unreachable: Option<RefCell<Vec<(u32, u32)>>>,
}

impl WalkFlow {
    /// A walk that collects the branches it proves cannot run.
    pub(crate) fn collecting_unreachable() -> Self {
        Self {
            unreachable: Some(RefCell::default()),
            ..Self::default()
        }
    }

    /// The ranges the walk proved cannot run, empty when it collected none.
    pub(crate) fn into_unreachable(self) -> Vec<(u32, u32)> {
        self.unreachable
            .map(RefCell::into_inner)
            .unwrap_or_default()
    }
}

/// Context for the forward walk.
///
/// Bundles the context that every statement/expression handler needs —
/// the class loader, function loader, current class info, source text,
/// etc.  The mutable `ScopeState` is passed separately as `&mut`; the
/// walk's control flow is the [`WalkFlow`] it borrows.
#[derive(Clone)]
pub(crate) struct ForwardWalkCtx<'a> {
    /// The file being walked.
    pub file: FileCtx<'a>,
    /// The class containing the method being analyzed (or a dummy for
    /// top-level functions).
    pub current_class: &'a ClassInfo,
    /// Byte offset of the cursor.  The walk stops when a statement's
    /// start offset reaches or exceeds this value.
    pub cursor_offset: u32,
    /// Cross-file loader callbacks (function loader, constant loader).
    pub loaders: Loaders<'a>,
    /// Shared cache of fully-resolved classes.
    pub resolved_class_cache: Option<&'a crate::virtual_members::ResolvedClassCache>,
    /// The `@return` type of the enclosing function/method, if known.
    /// Used for generator yield inference.
    pub enclosing_return_type: Option<PhpType>,
    /// Pre-computed top-level scope for resolving `global` variable imports.
    /// When a function body contains `global $x;`, the walker looks up
    /// `$x` in this map to seed the local scope with the top-level type.
    pub top_level_scope: Option<Locals>,
    /// Whether the statement currently being walked sits lexically inside a
    /// loop body (`for`/`foreach`/`while`/`do-while`, at any nesting depth).
    ///
    /// The fixed-point walk re-walks a loop body a handful of times to
    /// converge, not once per actual runtime iteration, so a statement
    /// inside one runs a statically unknowable number of times. Array
    /// writes read this to decide whether a `[]` append may keep a tracked
    /// shape's precise arity (safe outside a loop, where it runs exactly
    /// once) or must widen straight to the collection type it is building
    /// (required inside one, or the shape would grow by one entry per
    /// re-walk instead of settling). See
    /// `array_shape_writes::merge_nested_array_write`.
    pub in_loop: bool,
    /// The bounded `@template` parameters of the function or method whose
    /// body is being walked, each mapped to its
    /// [`TemplateParam`](crate::php_type::TypeKind::TemplateParam) marker.
    /// An inline `@var` naming one of them is read through this, so an
    /// `@var array<T>` inside the body knows what its elements are.
    pub template_markers: Option<Arc<HashMap<String, PhpType>>>,
    /// Where the expression walk records the scope it reached the cursor
    /// with, while it walks the statement that holds the cursor; see
    /// [`CursorScope`].
    pub cursor_scope: Option<&'a CursorScope>,
    /// The walk's control-flow state; see [`WalkFlow`].
    pub flow: &'a WalkFlow,
}

/// The scope at the cursor, as the expression walk found it.
///
/// A walk answering a question about one position (hover, completion,
/// go-to-definition) stops at the statement that holds the cursor, but the
/// cursor can sit deep inside that statement's expression: in the right
/// operand of `&&`, in a ternary arm, after a write the expression made
/// earlier.  The expression walk records the scope it evaluates each node
/// holding the cursor on, so what is left once the statement is walked is
/// the scope of the innermost one, and the statement walk answers from
/// that rather than from the scope around the statement.
#[derive(Default)]
pub(crate) struct CursorScope(std::cell::RefCell<Option<ScopeState>>);

impl CursorScope {
    /// Record `scope` as the one at the cursor, when `expr` holds it.
    pub(crate) fn reached(&self, expr: &Expression<'_>, scope: &ScopeState, cursor: u32) {
        let span = expr.span();
        if span.start.offset <= cursor && cursor <= span.end.offset {
            self.record(scope);
        }
    }

    /// Record `scope` as the one at the cursor.
    ///
    /// Kept out of line: the expression walk calls this at every level of
    /// a long `->method()` chain, and an inlined copy of the scope would
    /// grow every one of those frames.
    #[inline(never)]
    pub(crate) fn record(&self, scope: &ScopeState) {
        *self.0.borrow_mut() = Some(scope.clone());
    }

    /// The scope recorded last, if the walk reached the cursor.
    pub(crate) fn take(&self) -> Option<ScopeState> {
        self.0.borrow_mut().take()
    }
}

impl<'a> ForwardWalkCtx<'a> {
    /// A walk to `cursor_offset` of `file` inside `current_class`, with
    /// no loaders beyond the file's, no resolved-class cache and none of
    /// the per-body state (see [`Self::fresh`]).
    pub(crate) fn new(
        file: FileCtx<'a>,
        current_class: &'a ClassInfo,
        cursor_offset: u32,
        flow: &'a WalkFlow,
    ) -> Self {
        ForwardWalkCtx {
            file,
            current_class,
            cursor_offset,
            loaders: Loaders::default(),
            resolved_class_cache: None,
            enclosing_return_type: None,
            top_level_scope: None,
            in_loop: false,
            template_markers: None,
            cursor_scope: None,
            flow,
        }
    }

    /// Type `hint` against this walk's classes: the classes it names, or
    /// the bare type string when it names none.
    ///
    /// See [`crate::type_engine::type_resolution::resolved_types_for_hint`].
    pub(crate) fn resolved_types_for(&self, hint: PhpType) -> Vec<ResolvedType> {
        crate::type_engine::type_resolution::resolved_types_for_hint(
            hint,
            &self.current_class.fqn(),
            self.file.all_classes,
            self.file.class_loader,
        )
    }

    /// How a class name written at the cursor reads.
    pub(in crate::type_engine) fn written_names(&self) -> WrittenNames<'a> {
        self.file
            .written_names(Some(self.current_class), self.cursor_offset)
    }

    /// Build a walk context from a variable-resolution context.
    ///
    /// Lets the expression resolvers reach the narrowing pipeline that lives
    /// on this side of the walk (ternary arms, short-circuit operands) rather
    /// than re-deriving narrowing from syntax on their own.
    pub(crate) fn from_var_ctx(
        ctx: &crate::type_engine::resolver::VarResolutionCtx<'a>,
        flow: &'a WalkFlow,
    ) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            ..Self::fresh_from_var_ctx(ctx, flow)
        }
    }

    /// Build a walk context for a body of its own from a
    /// variable-resolution context: the same file, classes, loaders and
    /// cursor, but none of the per-body state (see [`Self::fresh`]).
    pub(crate) fn fresh_from_var_ctx(
        ctx: &crate::type_engine::resolver::VarResolutionCtx<'a>,
        flow: &'a WalkFlow,
    ) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            ..Self::new(ctx.file, ctx.current_class, ctx.cursor_offset, flow)
        }
    }

    /// Return a copy of this context for walking a body of its own: no
    /// enclosing return type, top-level scope, template markers or cursor
    /// scope, and not inside a loop.
    pub(crate) fn fresh(&self) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            enclosing_return_type: None,
            top_level_scope: None,
            in_loop: false,
            template_markers: None,
            cursor_scope: None,
            ..*self
        }
    }

    /// Return a copy of this context with a different `cursor_offset`.
    ///
    /// Used by the two-pass loop strategy: pass 1 runs with
    /// `cursor_offset = u32::MAX` so the entire loop body is walked
    /// and all assignments are discovered, even those after the real
    /// cursor position.
    pub(crate) fn with_cursor_offset(&self, cursor_offset: u32) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            cursor_offset,
            ..self.clone()
        }
    }

    /// Return a copy of this context marked as walking inside a loop body
    /// (or not). See [`ForwardWalkCtx::in_loop`].
    pub(crate) fn with_in_loop(&self, in_loop: bool) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            in_loop,
            ..self.clone()
        }
    }

    /// Return a copy of this context that records the scope at the cursor
    /// in `cursor_scope`, or records it nowhere.
    pub(crate) fn with_cursor_scope<'c>(
        &'c self,
        cursor_scope: Option<&'c CursorScope>,
    ) -> ForwardWalkCtx<'c> {
        ForwardWalkCtx {
            cursor_scope,
            ..self.clone()
        }
    }

    /// Return a copy of this context for walking the body of the
    /// declaration starting at `decl_start`, with that declaration's
    /// bounded templates as [`template_markers`](Self::template_markers)
    /// and its docblock's `@return` type as the
    /// [`enclosing_return_type`](Self::enclosing_return_type).
    pub(crate) fn for_declaration(&self, decl_start: u32) -> ForwardWalkCtx<'a> {
        use crate::type_engine::variable::resolution::{
            declaration_template_markers, declared_return_docblock_type,
        };
        let names = self.written_names();
        ForwardWalkCtx {
            template_markers: declaration_template_markers(
                self.file.content,
                decl_start as usize,
                &names,
            ),
            enclosing_return_type: declared_return_docblock_type(
                self.file.content,
                decl_start as usize,
            )
            .map(|ty| names.resolve_type(&ty)),
            top_level_scope: self.top_level_scope.clone(),
            ..*self
        }
    }

    /// Build a [`ResolutionCtx`](crate::type_engine::resolver::ResolutionCtx)
    /// from this walk context.
    ///
    /// Carries no variable resolver: it is for the resolutions that read
    /// the *declarations* around the walk (a constant behind a type
    /// operator, a class behind a name) rather than the values flowing
    /// through it.
    pub(crate) fn as_resolution_ctx(&self) -> ResolutionCtx<'_> {
        ResolutionCtx {
            function_loader: self.loaders.function_loader,
            resolved_class_cache: self.resolved_class_cache,
            ..ResolutionCtx::new(self.file, Some(self.current_class), self.cursor_offset)
        }
    }

    /// Build a [`VarResolutionCtx`] with a scope-based variable
    /// resolver.  Used by [`resolve_rhs_with_scope`] so that
    /// `resolve_rhs_expression` and its sub-functions read variable
    /// types from the forward walker's in-progress `ScopeState`
    /// instead of re-entering `resolve_variable_types`.
    pub(crate) fn var_ctx_for_with_scope<'b>(
        &'b self,
        var_name: &'b str,
        cursor_offset: u32,
        scope_resolver: &'b dyn Fn(&str) -> Vec<ResolvedType>,
        scope_proofs: Option<ScopeProofs<'b>>,
    ) -> VarResolutionCtx<'b>
    where
        'a: 'b,
    {
        VarResolutionCtx {
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            scope_var_resolver: Some(scope_resolver),
            scope_proofs,
            ..VarResolutionCtx::new(var_name, self.file, self.current_class, cursor_offset)
        }
    }
}
