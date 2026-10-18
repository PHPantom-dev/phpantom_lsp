//! Base class inheritance resolution.
//!
//! This module handles merging members from parent classes and traits
//! into a single `ClassInfo`.  The resulting merged class contains the
//! base set of members visible on an instance / static access,
//! respecting PHP's precedence rules:
//!
//!   class own > traits > parent chain
//!
//! `@mixin` members are handled separately by
//! [`PHPDocProvider`](crate::virtual_members::phpdoc::PHPDocProvider) in
//! the virtual member provider layer.
//!
//! This module also supports **generic type substitution**: when a child
//! class declares `@extends Parent<ConcreteType1, ConcreteType2>` and the
//! parent has `@template T1` / `@template T2`, the inherited methods and
//! properties have their template parameter references replaced with the
//! concrete types.

mod ancestor_generics;
pub mod ancestry;
pub mod enrichment;
pub mod generics;
mod member_types;
mod merge;
mod rebind;
pub mod traits;

use std::sync::Arc;

use crate::types::ClassInfo;

// Re-export functions that are used internally
pub(crate) use ancestor_generics::{
    extract_generic_arg_from_ancestor, extract_generic_args_from_ancestor,
};
pub(crate) use ancestry::{
    ancestors, find_declaring_ancestor, find_declaring_trait, supertypes, used_traits_deep,
};
pub(crate) use enrichment::enrich_method_arc_from_ancestor;
pub(crate) use enrichment::enrich_property_arc_from_ancestor;
pub(crate) use member_types::{resolve_method_return_type, resolve_property_type_hint};
pub(crate) use merge::{MergeDedup, TraitContext, resolve_class_with_inheritance};
pub(crate) use rebind::{is_extends_only_generic_rebindable, rebind_extends_only_generics};

#[cfg(test)]
pub(crate) use generics::apply_substitution;
pub(crate) use generics::{
    apply_generic_args, apply_substitution_to_conditional, apply_substitution_to_method,
    apply_substitution_to_property, bind_inherited_class_keywords,
    bind_inherited_class_keywords_in_property, bind_template_args, build_generic_subs,
    build_substitution_map, class_scoped_template_values, default_type_args, extends_type_args,
    fill_template_bounds, generic_arg_offset, method_has_inherited_class_keyword,
    method_references_params, property_has_inherited_class_keyword, property_references_params,
    template_values_with_defaults,
};

/// A borrow-or-owned handle to a `ClassInfo`, used to walk the parent
/// chain in [`resolve_class_with_inheritance`] without cloning the root
/// class.
///
/// The first iteration borrows the caller-provided `&ClassInfo` (zero
/// allocation).  Subsequent iterations hold the `Arc<ClassInfo>` returned
/// by the class loader (a cheap Arc move).
pub(crate) enum ClassRef<'a> {
    Borrowed(&'a ClassInfo),
    Owned(Arc<ClassInfo>),
}

impl std::ops::Deref for ClassRef<'_> {
    type Target = ClassInfo;
    #[inline]
    fn deref(&self) -> &ClassInfo {
        match self {
            ClassRef::Borrowed(r) => r,
            ClassRef::Owned(a) => a,
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "inheritance_tests.rs"]
mod tests;
