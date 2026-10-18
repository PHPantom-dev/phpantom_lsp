//! Data types used throughout the PHPantom server: the model of the PHP
//! code that was parsed.
//!
//! Each kind of declaration has a file of its own ([`ClassInfo`] in
//! `class_info`, [`MethodInfo`] in `method`, [`PropertyInfo`] in
//! `property`, [`ConstantInfo`] in `constant`, [`FunctionInfo`] in
//! `function`, [`ParameterInfo`] in `parameter`), and everything is
//! re-exported from here, so callers name it as `crate::types::…`. This
//! file keeps the few small types several declarations share.

// Re-export SubjectExpr and BracketSegment from their canonical module
// so that existing `use crate::types::{SubjectExpr, BracketSegment, …}`
// paths continue to work.
pub use crate::type_engine::subject_expr::{BracketSegment, SubjectExpr};

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::atom::{Atom, AtomMap};
use crate::php_type::{PhpType, TypeKind};

mod access;
mod assertions;
mod class_info;
mod constant;
mod file_context;
mod function;
mod laravel;
mod method;
mod parameter;
mod php_version;
mod property;
mod resolved_type;
mod shared_vec;

pub use access::*;
pub use assertions::*;
pub use class_info::*;
pub use constant::*;
pub use file_context::*;
pub use function::*;
pub use laravel::*;
pub use method::*;
pub use parameter::*;
pub use php_version::*;
pub use property::*;
pub use resolved_type::*;
pub use shared_vec::*;

/// A type a declaration's docblock settled in place of its native hint
/// while naming a class the hint does not.
///
/// PHP enforces the native hint, so the docblock type may only narrow it,
/// and whether a class does takes the class hierarchy: `@return Clock` on
/// `: \DateTimeInterface` narrows it when `Clock` implements the interface,
/// and `@return DateTimeImmutable` inside `namespace App` names an
/// `App\DateTimeImmutable` that may not exist at all.  The parser has no
/// class loader, so it records the type here, and loading the declaration
/// decides (see `type_engine::types::docblock_narrowing`).
#[derive(Debug, Clone, PartialEq)]
pub struct UnverifiedDocType {
    /// The method or property the type belongs to, or the function itself.
    pub member: Atom,
    /// Which of the member's types it is.
    pub slot: DocTypeSlot,
    /// What the slot holds when the docblock type does not narrow the
    /// native hint: the hint with its class names qualified, and with `null`
    /// for a parameter that defaults to it.
    pub native: PhpType,
}

/// Where in a declaration an [`UnverifiedDocType`] was settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DocTypeSlot {
    /// The return type.
    Return,
    /// The parameter at this position.
    Param(u32),
    /// A property's type.
    Property,
}

/// Variance of a `@template` parameter.
///
/// PHPStan and Psalm support `@template-covariant` and
/// `@template-contravariant` to express variance constraints on generic
/// type parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TemplateVariance {
    /// No variance annotation (`@template T`).
    #[default]
    Invariant,
    /// `@template-covariant T`
    Covariant,
    /// `@template-contravariant T`
    Contravariant,
}

impl TemplateVariance {
    /// Returns the tag name used in PHPDoc for this variance.
    pub fn tag_name(self) -> &'static str {
        match self {
            Self::Invariant => "template",
            Self::Covariant => "template-covariant",
            Self::Contravariant => "template-contravariant",
        }
    }
}

/// Visibility of a class member (method, property, or constant).
///
/// In PHP, members without an explicit visibility modifier default to `Public`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Public,
    Protected,
    Private,
}

// ─── Recursion Depth Limits ─────────────────────────────────────────────────
//
// Centralised constants for the maximum recursion depth allowed when
// walking inheritance chains, trait hierarchies, mixin graphs, and type
// alias resolution.  Defining them in one place ensures that the same
// limit is used consistently across the inheritance, definition, and
// completion modules.

/// Maximum depth when walking the `extends` parent chain
/// (class → parent → grandparent → …).
pub(crate) const MAX_INHERITANCE_DEPTH: u32 = 20;

/// Maximum depth when recursing into `use Trait` hierarchies
/// (a trait can itself `use` other traits).
pub(crate) const MAX_TRAIT_DEPTH: u32 = 20;

/// Maximum depth when recursing into `@mixin` class graphs.
pub(crate) const MAX_MIXIN_DEPTH: u32 = 10;

/// Maximum depth when resolving `@phpstan-type` / `@psalm-type` aliases
/// (an alias can reference another alias).
pub(crate) const MAX_ALIAS_DEPTH: u8 = 10;

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "../types_tests.rs"]
mod tests;
