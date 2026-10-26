//! Operations on types that do not walk a function body.
//!
//! This group contains:
//! - **aliases**: expanding `@phpstan-type` aliases into member signatures
//! - **docblock_narrowing**: keeping a docblock class only where it narrows
//!   the native hint, which takes the class hierarchy
//! - **resolution**: Type-hint string to `ClassInfo` mapping (unions,
//!   intersections, generics, type aliases, object shapes, property types)
//! - **condition_facts**: what an instanceof check, an assertion, or a
//!   type-check call says about a variable
//! - **conditional**: PHPStan conditional return type resolution at call sites
//! - **call_site_returns**: builtin return types decided by a call's
//!   arguments (a flag bit, an options array, an object argument), which a
//!   conditional return type cannot express
//! - **const_fold**: folding a constant expression to the value PHP would
//!   compute for it, so a constant defined from other constants is readable
//! - **int_interval**: the bounds of an integer type, which arithmetic on
//!   `int<a, b>` operands and a loop's widening of a growing counter use

pub mod aliases;
pub mod call_site_returns;
pub mod condition_facts;
pub mod conditional;
pub mod const_fold;
pub mod docblock_narrowing;
pub(crate) mod int_interval;
pub mod resolution;
