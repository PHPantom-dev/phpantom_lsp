//! The immutable context a forward walk carries: loaders, the enclosing
//! class and function, the source text, and the per-walk settings every
//! statement and expression handler reads.

use super::*;
use std::sync::Arc;

use crate::atom::AtomMap;
use crate::php_type::PhpType;
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::types::{ClassInfo, ResolvedType};

/// Context for the forward walk.
///
/// Bundles the immutable context that every statement/expression handler
/// needs — the class loader, function loader, current class info, source
/// text, etc.  The mutable `ScopeState` is passed separately as `&mut`.
pub(crate) struct ForwardWalkCtx<'a> {
    /// The class containing the method being analyzed (or a dummy for
    /// top-level functions).
    pub current_class: &'a ClassInfo,
    /// All classes known in the current file.
    pub all_classes: &'a [Arc<ClassInfo>],
    /// Full source text of the current file.
    pub content: &'a str,
    /// Byte offset of the cursor.  The walk stops when a statement's
    /// start offset reaches or exceeds this value.
    pub cursor_offset: u32,
    /// Cross-file class resolution callback.
    pub class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    /// Server state for project-wide answers.  See
    /// [`ResolutionCtx::backend`](crate::type_engine::resolver::ResolutionCtx::backend).
    pub backend: Option<&'a crate::Backend>,
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
    pub top_level_scope: Option<AtomMap<Vec<ResolvedType>>>,
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
}

impl<'a> ForwardWalkCtx<'a> {
    /// Type `hint` against this walk's classes: the classes it names, or
    /// the bare type string when it names none.
    ///
    /// See [`crate::type_engine::type_resolution::resolved_types_for_hint`].
    pub(crate) fn resolved_types_for(&self, hint: PhpType) -> Vec<ResolvedType> {
        crate::type_engine::type_resolution::resolved_types_for_hint(
            hint,
            &self.current_class.name,
            self.all_classes,
            self.class_loader,
        )
    }

    /// Build a walk context from a variable-resolution context.
    ///
    /// Lets the expression resolvers reach the narrowing pipeline that lives
    /// on this side of the walk (ternary arms, short-circuit operands) rather
    /// than re-deriving narrowing from syntax on their own.
    pub(crate) fn from_var_ctx(
        ctx: &crate::type_engine::resolver::VarResolutionCtx<'a>,
    ) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            current_class: ctx.current_class,
            all_classes: ctx.all_classes,
            content: ctx.content,
            cursor_offset: ctx.cursor_offset,
            class_loader: ctx.class_loader,
            backend: ctx.backend,
            loaders: ctx.loaders,
            resolved_class_cache: ctx.resolved_class_cache,
            enclosing_return_type: ctx.enclosing_return_type.clone(),
            top_level_scope: ctx.top_level_scope.clone(),
            in_loop: false,
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
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset,
            class_loader: self.class_loader,
            backend: self.backend,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            in_loop: self.in_loop,
        }
    }

    /// Return a copy of this context marked as walking inside a loop body
    /// (or not). See [`ForwardWalkCtx::in_loop`].
    pub(crate) fn with_in_loop(&self, in_loop: bool) -> ForwardWalkCtx<'a> {
        ForwardWalkCtx {
            current_class: self.current_class,
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset: self.cursor_offset,
            class_loader: self.class_loader,
            backend: self.backend,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            in_loop,
        }
    }

    /// Build a [`ResolutionCtx`](crate::type_engine::resolver::ResolutionCtx)
    /// from this walk context.
    ///
    /// Carries no variable resolver: it is for the resolutions that read
    /// the *declarations* around the walk (a constant behind a type
    /// operator, a class behind a name) rather than the values flowing
    /// through it.
    pub(crate) fn as_resolution_ctx(&self) -> crate::type_engine::resolver::ResolutionCtx<'_> {
        crate::type_engine::resolver::ResolutionCtx {
            current_class: Some(self.current_class),
            all_classes: self.all_classes,
            content: self.content,
            cursor_offset: self.cursor_offset,
            class_loader: self.class_loader,
            backend: self.backend,
            laravel_macro_this_resolver: None,
            function_loader: self.loaders.function_loader,
            resolved_class_cache: self.resolved_class_cache,
            scope_var_resolver: None,
            is_in_static_method: false,
            preserve_static: false,
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
            backend: self.backend,
            loaders: self.loaders,
            resolved_class_cache: self.resolved_class_cache,
            enclosing_return_type: self.enclosing_return_type.clone(),
            top_level_scope: self.top_level_scope.clone(),
            scope_var_resolver: Some(scope_resolver),
            scope_proofs,
            ..VarResolutionCtx::new(
                var_name,
                self.current_class,
                self.all_classes,
                self.content,
                cursor_offset,
                self.class_loader,
            )
        }
    }
}
