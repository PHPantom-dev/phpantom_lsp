//! What a runtime check says about a variable's type.
//!
//! This module reads a condition and works out the narrowing it implies:
//! the facts, and how they apply to a candidate type or class list.  The
//! forward walker's `cond_narrowing` module applies them to the scope at
//! each branch.  Supported patterns include:
//!
//!   - `if ($var instanceof ClassName)` — narrows inside the then-body
//!   - `if (!$var instanceof ClassName)` — narrows inside the else-body
//!   - `is_a($var, ClassName::class)` — equivalent to instanceof
//!   - `get_class($var) === ClassName::class` — exact class identity check
//!   - `$var::class === ClassName::class` — exact class identity check
//!   - `assert($var instanceof ClassName)` — unconditional narrowing
//!   - `@phpstan-assert` / `@psalm-assert` — custom type guard functions
//!   - `match(true) { $var instanceof Foo => … }` — match-arm narrowing
//!   - `$var instanceof Foo ? $var->method() : …` — ternary narrowing
//!   - `$var instanceof Foo && $var->method()` — inline `&&` narrowing
//!     (the RHS of `&&` sees the narrowed type from the LHS)
//!   - `!$var instanceof Foo || $var->method()` — inline `||`
//!     short-circuit narrowing (the RHS of `||` sees the *inverse* of
//!     the LHS, so `$var` is `Foo` where the right operand executes)
//!   - Guard clauses: `if (!$var instanceof Foo) { return; }` — narrows
//!     after the if block when the body unconditionally exits via
//!     `return`, `throw`, `continue`, or `break`.
//!   - `in_array($var, $haystack, true)` — narrows `$var` to the
//!     haystack's element type when the third argument is `true`.
//!   - `is_array($var)` — narrows to only the array-like members of a
//!     union type, preserving generic element types from PHPDoc.
//!   - `is_string($var)`, `is_int($var)`, `is_bool($var)`, etc. —
//!     narrows to the corresponding scalar type.

mod assert_annotations;
mod builtin_checks;
mod class_checks;
mod conditional_returns;
mod exits;
mod resolve;

pub(in crate::type_engine) use assert_annotations::*;
pub(in crate::type_engine) use builtin_checks::*;
pub(in crate::type_engine) use class_checks::*;
pub(in crate::type_engine) use conditional_returns::*;
pub(in crate::type_engine) use exits::*;
pub(in crate::type_engine) use resolve::*;
// The symbol-map extractor builds the same bracket-index text the
// narrowing keys use, so the two sides cannot drift apart.
pub(crate) use resolve::array_index_key;
// The unreachable-code diagnostic asks the same "does control leave this
// list?" question without types, so it shares the structural rules.
pub(crate) use exits::{contains_entry_label, statement_leaves_block};
