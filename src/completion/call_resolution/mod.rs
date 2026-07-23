//! Call expression and callable target resolution.
//!
//! ## Callable target cache
//!
//! During diagnostic passes, `resolve_instance_method_callable` is
//! called for every call site in the file.  Many different chain
//! expressions resolve to the same (class, method) pair — e.g.
//! `$q->where(...)`, `$query->where(...)`, and
//! `Product::query()->where(...)` all end up looking for `where` on
//! `Builder<Product>`.  The per-file callable-target cache
//! (`CALLABLE_TARGET_CACHE`) stores `Option<ResolvedCallableTarget>`
//! keyed by `(class_fqn, method_name_lower)` so these redundant
//! resolutions are free after the first hit.
///
/// This module contains the logic for resolving call expressions (method
/// calls, static calls, function calls, constructor calls) to their
/// return types, as well as resolving callable targets for signature help
/// and named-argument completion.
///
/// Split from [`super::resolver`] for navigability. The entry points are:
///
/// - [`Backend::resolve_callable_target`]: resolves a call expression
///   string to a [`ResolvedCallableTarget`] with label, parameters, and
///   return type (used by signature help and named-argument completion).
/// - [`Backend::resolve_call_return_types_expr_with_hint`]: resolves the return
///   type of a structured [`SubjectExpr`] callee + argument text to
///   zero or more `ClassInfo` values (used by the completion chain).
/// - [`Backend::resolve_method_return_types_with_args`]: resolves a
///   method's return type on a specific class, handling conditional
///   return types and template substitutions.
/// - [`Backend::build_method_template_subs`]: builds a template
///   substitution map for method-level `@template` parameters from
///   pre-split call-site argument texts.
///
/// The logic is spread across sibling files:
///
/// - [`target_cache`]: thread-local caches and RAII activation guards
///   (callable target cache, body-return-type inference memo, guard-aware
///   auth user resolver).
/// - [`callable_target`]: resolving a call expression to a
///   [`ResolvedCallableTarget`] (signature help, named-argument completion).
/// - [`return_types`]: the primary call return-type resolution entry
///   point, plus the auth/date facade helpers and literal/expression-to-type
///   conversions it depends on.
/// - [`template_subs`]: building a method-level `@template` substitution
///   map from call-site argument texts.
/// - [`arg_type_resolution`]: resolving inline argument expressions to
///   their raw `PhpType`.
mod arg_type_resolution;
mod callable_target;
mod return_types;
mod target_cache;
mod template_subs;

pub(crate) use return_types::MethodReturnCtx;
pub(crate) use target_cache::{try_infer_body_return_type, with_callable_target_cache};
