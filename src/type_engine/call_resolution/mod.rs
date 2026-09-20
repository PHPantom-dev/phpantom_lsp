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
//!
//! This module contains the logic for resolving call expressions (method
//! calls, static calls, function calls, constructor calls) to their
//! return types, as well as resolving callable targets for signature help
//! and named-argument completion.
//!
//! Split from [`super::resolver`] for navigability. The entry points are:
//!
//! - [`Backend::resolve_callable_target`]: resolves a call expression
//!   string to a [`ResolvedCallableTarget`] with label, parameters, and
//!   return type (used by signature help and named-argument completion).
//! - [`Backend::resolve_call_return_types_expr_with_hint`]: resolves the return
//!   type of a structured [`SubjectExpr`] callee + argument text to
//!   zero or more `ClassInfo` values (used by the completion chain).
//! - [`Backend::resolve_method_return_types_with_args`]: resolves a
//!   method's return type on a specific class, handling conditional
//!   return types and template substitutions.
//! - [`Backend::build_method_template_subs`]: builds a template
//!   substitution map for method-level `@template` parameters from
//!   pre-split call-site argument texts.
//!
//! The logic is spread across sibling files:
//!
//! - [`target_cache`]: the request-scoped memos (callable target cache,
//!   body-return-type inference memo) and the body-return-type inference
//!   they serve.  The memos are activated as a unit by
//!   `activate_type_engine_caches`; the project-wide facilities the type
//!   engine consults (body-return inference, auth guard models,
//!   validation rules) come off the `Backend` carried on the resolution
//!   context, so every feature resolves an expression with the same
//!   facilities available.
//! - [`callable_target`]: resolving a call expression to a
//!   [`ResolvedCallableTarget`] (signature help, named-argument completion).
//! - [`return_types`]: the primary call return-type resolution entry
//!   point, plus the auth/date facade helpers and literal/expression-to-type
//!   conversions it depends on.
//! - [`template_subs`]: building a method-level `@template` substitution
//!   map from call-site argument texts.
//! - [`arg_type_resolution`]: resolving inline argument expressions to
//!   their raw `PhpType`.
//! - [`facade_owner`]: picking the concrete container class that types a
//!   static call made through a Laravel facade.
//! - [`out_param`]: what a callee leaves in a by-reference parameter,
//!   read out of its body when the declaration is wider than what the
//!   body assigns.
//! - [`reflection`]: typing a property read through the Reflection API,
//!   whose result depends on the property name passed at the call site.
mod arg_type_resolution;
mod callable_target;
mod facade_owner;
mod out_param;
mod reflection;
mod return_types;
mod target_cache;
mod template_subs;

pub(crate) use out_param::{OutParamCallee, effective_out_type};

pub(crate) use facade_owner::facade_concrete_owner;
pub(crate) use reflection::{
    is_reflected_property_call, is_reflected_property_class, resolve_reflected_property_at_call,
    resolve_reflected_property_at_new,
};
pub(crate) use return_types::{
    MethodReturnCtx, folded_class_constant_type, folded_global_constant_type,
    resolve_static_access_type,
};
pub(crate) use target_cache::{
    activate_type_engine_caches, body_inference_in_progress, call_site_param_types,
    try_infer_body_return_type,
};
pub(crate) use template_subs::{
    array_literal_shape_type, bind_callable_param_template, bind_callable_return_template,
    build_call_template_subs, evaluate_constant_operands, finish_template_subs,
    type_operator_bound_literal,
};
