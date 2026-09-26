/// Argument-text-to-type resolution: converts call-site argument texts
/// (literals, array shapes, variables, chained expressions) to `PhpType`
/// for template substitution and generic argument binding.
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::find_class_by_name;
use crate::class_lookup::resolve_class_keyword;
use crate::docblock;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::*;

use crate::type_engine::conditional_resolution::split_text_args;
use crate::type_engine::resolver::{Loaders, ResolutionCtx};
use crate::type_engine::subject_expr::split_call_subject_raw;
use crate::type_engine::variable::array_func_rules::ArrayFuncArgs;

thread_local! {
    /// Re-entry guard for [`Backend::resolve_inline_arg_raw_type`].
    /// Tracks the argument texts currently being resolved on this stack.
    ///
    /// Resolving an inline argument runs the full variable-resolution
    /// pipeline over the entire enclosing file at the caller's cursor
    /// offset.  When that argument is itself a nested call expression
    /// (e.g. `array_map(...)` nested inside `array_filter(...)`), the
    /// re-walk re-reaches the same enclosing call and asks for the same
    /// argument's raw type again.  Because the re-walk always covers the
    /// whole program at the same cursor, this cycle is not bounded by the
    /// expression's finite nesting depth and recurses until the stack
    /// overflows.  Keying by argument text breaks the cycle on the second
    /// entry while leaving distinct arguments (and sequential resolution
    /// of identically-spelled sibling arguments) unaffected.
    static INLINE_ARG_RESOLVING: RefCell<HashSet<String>> =
        RefCell::new(HashSet::new());
}

/// RAII guard that removes an argument text from [`INLINE_ARG_RESOLVING`]
/// on drop, so the many early returns in `resolve_inline_arg_raw_type`
/// cannot leak an in-flight key.
struct InlineArgResolvingGuard {
    key: String,
}

impl Drop for InlineArgResolvingGuard {
    fn drop(&mut self) {
        INLINE_ARG_RESOLVING.with(|cell| {
            cell.borrow_mut().remove(&self.key);
        });
    }
}

/// [`ArrayFuncArgs`] over a call's raw argument source text.
///
/// The counterpart to the AST implementation in
/// `type_engine::variable::raw_type_inference`; both feed the shared
/// rules in [`crate::type_engine::variable::array_func_rules`] so an
/// inline `array_map(…)` gets the same element type as one assigned to a
/// variable.
pub(super) struct TextArrayFuncArgs<'a, 'ctx> {
    args: Vec<&'a str>,
    ctx: &'a ResolutionCtx<'ctx>,
}

impl<'a, 'ctx> TextArrayFuncArgs<'a, 'ctx> {
    pub(super) fn new(text_args: &'a str, ctx: &'a ResolutionCtx<'ctx>) -> Self {
        Self {
            args: split_text_args(text_args),
            ctx,
        }
    }

    /// The nth argument's value text, with any `name:` prefix removed
    /// so that a named argument reads the same as a positional one.
    fn arg_text(&self, index: usize) -> Option<&'a str> {
        self.args
            .get(index)
            .map(|arg| crate::call_args::text_arg_value(arg))
    }
}

impl ArrayFuncArgs for TextArrayFuncArgs<'_, '_> {
    fn arg_raw_type(&self, index: usize) -> Option<PhpType> {
        Backend::resolve_inline_arg_raw_type(self.arg_text(index)?, self.ctx)
    }

    fn bool_literal(&self, index: usize) -> Option<bool> {
        let arg = self.arg_text(index)?;
        if arg.eq_ignore_ascii_case("true") {
            Some(true)
        } else if arg.eq_ignore_ascii_case("false") {
            Some(false)
        } else {
            None
        }
    }

    fn has_arg(&self, index: usize) -> bool {
        self.arg_text(index).is_some_and(|arg| !arg.is_empty())
    }

    fn is_spread(&self, index: usize) -> bool {
        self.arg_text(index)
            .is_some_and(|arg| arg.trim_start().starts_with("..."))
    }

    fn callback_declared_return_type(&self, index: usize) -> Option<PhpType> {
        let text = self.arg_text(index)?;
        crate::completion::source::helpers::extract_closure_return_type_from_text(text)
            // A `: ReturnType` annotation is read as written, so it still
            // carries the file's own spelling of the class (`Support\Pen`
            // behind a `use App\Support;`).  It is compared against types
            // that arrived fully qualified, so canonicalise it here.
            .map(|ty| crate::util::resolve_php_type_names(&ty, self.ctx.class_loader))
            .or_else(|| {
                // See the AST counterpart: a callable string names a
                // function whose declared return the call hands back.
                let name =
                    crate::type_engine::variable::array_func_rules::callable_string_function_name(
                        text,
                    )?;
                (self.ctx.function_loader?)(name, 0)?.return_type
            })
            .or_else(|| {
                // A first-class callable (`Row::fromCache(...)`) returns
                // what the method it names returns.
                text.trim_end().ends_with("(...)").then_some(())?;
                crate::completion::source::helpers::resolve_first_class_callable_return_type(
                    text, self.ctx,
                )
            })
    }

    fn callback_inferred_return_type(&self, index: usize, param_type: &PhpType) -> Option<PhpType> {
        Backend::infer_closure_return_type_from_body(self.arg_text(index)?, param_type, self.ctx)
    }

    fn arg_atom_text(&self, index: usize) -> Option<String> {
        let text = self.arg_text(index)?.trim();
        let atom = crate::util::strip_fqn_prefix(text);
        let is_atom =
            !atom.is_empty() && atom.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        is_atom.then(|| atom.to_string())
    }

    fn callback_param_narrowing(
        &self,
        index: usize,
        param_index: usize,
        subject: &PhpType,
    ) -> Option<PhpType> {
        crate::type_engine::variable::callback_narrowing::narrow_callback_param_text(
            self.arg_text(index)?,
            param_index,
            subject,
            Some(&self.ctx.class_loader),
        )
    }

    fn narrows(&self, inferred: &PhpType, declared: &PhpType) -> bool {
        crate::class_lookup::is_subtype_of_typed(inferred, declared, self.ctx.class_loader)
    }

    fn guard_split(&self, guard: &str, subject: &PhpType) -> Option<(Option<PhpType>, bool)> {
        crate::type_engine::types::narrowing::split_type_by_guard_name(
            guard,
            subject,
            Some(&self.ctx.class_loader),
        )
    }
}

impl Backend {
    /// Extract the first argument from a comma-separated argument text.
    ///
    /// [`split_text_args`] does the splitting: it respects nested
    /// brackets and, unlike the hand-rolled scan this replaced, string
    /// literals too, so a comma inside `'a, b'` no longer ends the first
    /// argument.
    pub(super) fn extract_first_arg_text(args_text: &str) -> Option<String> {
        let first = split_text_args(args_text).into_iter().next()?.trim();
        (!first.is_empty()).then(|| crate::call_args::text_arg_value(first).to_string())
    }

    /// Resolve the raw return type of an inline argument expression.
    ///
    /// Handles plain variables (`$customers`), call chains
    /// (`Customer::get()->all()`), and static calls (`ClassName::method()`).
    ///
    /// Returns the structured type (e.g. `array<int, Customer>`) so
    /// that the caller can extract element types from it.
    pub(super) fn resolve_inline_arg_raw_type(
        arg_text: &str,
        ctx: &ResolutionCtx<'_>,
    ) -> Option<PhpType> {
        // Break re-entrant resolution of the same argument text.  This
        // function re-walks the whole enclosing program at the caller's
        // cursor, so a nested call-expression argument re-reaches the same
        // call and re-requests its own raw type; without this guard that
        // cycle recurses until the stack overflows (nested `array_map` /
        // `array_filter` chains being the common trigger).
        let newly_inserted =
            INLINE_ARG_RESOLVING.with(|cell| cell.borrow_mut().insert(arg_text.to_string()));
        if !newly_inserted {
            return None;
        }
        let _guard = InlineArgResolvingGuard {
            key: arg_text.to_string(),
        };

        let current_class = ctx.current_class;
        let all_classes = ctx.all_classes;
        let class_loader = ctx.class_loader;

        // ── Plain variable: `$customers` ────────────────────────────────
        if arg_text.starts_with('$')
            && arg_text[1..]
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_')
        {
            // The walker's scope comes first: it knows which guard the
            // call sits behind, while the backward `@var` scan below
            // describes the variable at the annotation, before any
            // narrowing the guard proved.
            let from_scope = crate::type_engine::variable::resolution::walker_scope_types(
                arg_text,
                ctx.cursor_offset,
                ctx.scope_var_resolver,
            );
            if !from_scope.is_empty() {
                let joined = ResolvedType::types_joined(&from_scope);
                if joined.extract_value_type(false).is_some() {
                    return Some(joined);
                }
            }

            // Try docblock annotation (@var / @param).
            if let Some(raw) = docblock::find_iterable_raw_type_in_source(
                ctx.content,
                ctx.cursor_offset as usize,
                arg_text,
            )
            .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader))
            {
                // A bare identifier that isn't a known keyword type and
                // doesn't resolve to a loadable class is most likely an
                // unbound method-level `@template` parameter (e.g.
                // `@template T of Token[]` used as `@param T $tokens`).
                // The raw scan above is a text-only lookup that doesn't
                // apply template-bound substitution, so trusting it here
                // would leave the caller with an unresolvable `T` instead
                // of the array-of-`Token` type the forward walker would
                // produce.  Fall through to the unified pipeline below,
                // which resolves the variable through the forward walker
                // and substitutes template params with their bounds.
                let looks_like_unbound_template = match &raw.kind() {
                    TypeKind::Named(name) => {
                        !crate::php_type::is_keyword_type(name)
                            && (ctx.class_loader)(name).is_none()
                    }
                    _ => false,
                };
                if !looks_like_unbound_template {
                    return Some(raw);
                }
            }
            // Fall back to the unified variable resolution pipeline.
            let default_class;
            let effective_class = match current_class {
                Some(cc) => cc,
                None => {
                    default_class = crate::class_lookup::class_context_placeholder(
                        ctx.content,
                        ctx.cursor_offset,
                    );
                    &default_class
                }
            };
            let resolved = crate::type_engine::variable::resolution::resolve_variable_types(
                arg_text,
                effective_class,
                all_classes,
                ctx.content,
                ctx.cursor_offset,
                class_loader,
                ctx.backend,
                Loaders::with_function(ctx.function_loader),
            );
            if !resolved.is_empty() {
                return Some(ResolvedType::types_joined(&resolved));
            }
            return None;
        }

        // ── Call expression ending with `)` ─────────────────────────────
        if arg_text.ends_with(')')
            && let Some((call_body, call_args)) = split_call_subject_raw(arg_text)
        {
            match &SubjectExpr::parse_callee(call_body) {
                // A nested function call (e.g. `array_map($cb,
                // iterator_to_array($it))`) needs the full return-type
                // resolution rather than a declared type hint: the
                // element-type rules for the array-producing standard
                // library functions, conditional return types and
                // function-level `@template` substitution all report
                // through the hint.
                callee @ SubjectExpr::FunctionCall(_) => {
                    let mut hint: Option<PhpType> = None;
                    let _ = Backend::resolve_call_return_types_expr_with_hint(
                        callee,
                        call_args,
                        ctx,
                        Some(&mut hint),
                    );
                    if hint.is_some() {
                        return hint;
                    }
                }
                SubjectExpr::MethodCall { base, method } => {
                    let base_text = base.to_subject_text();
                    let lhs_classes = ResolvedType::into_arced_classes(
                        crate::type_engine::resolver::resolve_target_classes(
                            &base_text,
                            AccessKind::Arrow,
                            ctx,
                        ),
                    );
                    for cls in &lhs_classes {
                        if let Some(rt) = crate::inheritance::resolve_method_return_type(
                            cls,
                            method,
                            class_loader,
                        ) {
                            return Some(rt);
                        }
                    }
                }
                SubjectExpr::StaticMethodCall { class, method } => {
                    let owner = if let Some(resolved) = resolve_class_keyword(class, current_class)
                    {
                        class_loader(&resolved).map(Arc::unwrap_or_clone)
                    } else {
                        find_class_by_name(all_classes, class)
                            .map(|arc| ClassInfo::clone(arc))
                            .or_else(|| class_loader(class).map(Arc::unwrap_or_clone))
                    };
                    if let Some(ref cls) = owner
                        && let Some(rt) = crate::inheritance::resolve_method_return_type(
                            cls,
                            method,
                            class_loader,
                        )
                    {
                        return Some(rt);
                    }
                }
                _ => {}
            }
        }

        // ── Property access: `$this->prop` or `$var->prop` ──────────────
        if let Some(pos) = arg_text.rfind("->") {
            // Strip trailing `?` from LHS when the operator was `?->`
            let lhs = arg_text[..pos]
                .strip_suffix('?')
                .unwrap_or(&arg_text[..pos]);
            let prop_name = &arg_text[pos + 2..];
            if !prop_name.is_empty() && prop_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let lhs_classes = ResolvedType::into_arced_classes(
                    crate::type_engine::resolver::resolve_target_classes(
                        lhs,
                        AccessKind::Arrow,
                        ctx,
                    ),
                );
                for cls in &lhs_classes {
                    if let Some(rt) =
                        crate::inheritance::resolve_property_type_hint(cls, prop_name, class_loader)
                    {
                        return Some(rt);
                    }
                }
            }
        }

        None
    }
}
