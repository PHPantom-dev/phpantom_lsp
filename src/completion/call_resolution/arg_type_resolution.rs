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
use crate::php_type::PhpType;
use crate::subject_expr::SubjectExpr;
use crate::types::*;

use crate::completion::conditional_resolution::split_call_subject;
use crate::completion::resolver::{Loaders, ResolutionCtx};

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

impl Backend {
    /// Extract the first argument from a comma-separated argument text,
    /// respecting nested parentheses, brackets, and braces.
    pub(super) fn extract_first_arg_text(args_text: &str) -> Option<String> {
        let trimmed = args_text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut depth = 0i32;
        for (i, ch) in trimmed.char_indices() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => {
                    let arg = trimmed[..i].trim();
                    if !arg.is_empty() {
                        return Some(arg.to_string());
                    }
                    return None;
                }
                _ => {}
            }
        }
        // Single (or last) argument.
        let arg = trimmed.trim();
        if !arg.is_empty() {
            Some(arg.to_string())
        } else {
            None
        }
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
            // Try docblock annotation first (@var / @param).
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
                let looks_like_unbound_template = match &raw {
                    PhpType::Named(name) => {
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
            let default_class = ClassInfo::default();
            let effective_class = current_class.unwrap_or(&default_class);
            let resolved = crate::completion::variable::resolution::resolve_variable_types(
                arg_text,
                effective_class,
                all_classes,
                ctx.content,
                ctx.cursor_offset,
                class_loader,
                Loaders::with_function(ctx.function_loader),
            );
            if !resolved.is_empty() {
                return Some(ResolvedType::types_joined(&resolved));
            }
            return None;
        }

        // ── Call expression ending with `)` ─────────────────────────────
        if arg_text.ends_with(')')
            && let Some((call_body, _args)) = split_call_subject(arg_text)
        {
            match SubjectExpr::parse_callee(call_body) {
                SubjectExpr::MethodCall { base, method } => {
                    let base_text = base.to_subject_text();
                    let lhs_classes = ResolvedType::into_arced_classes(
                        crate::completion::resolver::resolve_target_classes(
                            &base_text,
                            AccessKind::Arrow,
                            ctx,
                        ),
                    );
                    for cls in &lhs_classes {
                        if let Some(rt) = crate::inheritance::resolve_method_return_type(
                            cls,
                            &method,
                            class_loader,
                        ) {
                            return Some(rt);
                        }
                    }
                }
                SubjectExpr::StaticMethodCall { class, method } => {
                    let owner = if let Some(resolved) = resolve_class_keyword(&class, current_class)
                    {
                        class_loader(&resolved).map(Arc::unwrap_or_clone)
                    } else {
                        find_class_by_name(all_classes, &class)
                            .map(|arc| ClassInfo::clone(arc))
                            .or_else(|| class_loader(&class).map(Arc::unwrap_or_clone))
                    };
                    if let Some(ref cls) = owner
                        && let Some(rt) = crate::inheritance::resolve_method_return_type(
                            cls,
                            &method,
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
                    crate::completion::resolver::resolve_target_classes(
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
