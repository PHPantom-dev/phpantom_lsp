//! Type narrowing for variable resolution.
//!
//! This module contains the logic for narrowing a variable's type based on
//! runtime checks that appear before the cursor position.  Supported
//! patterns include:
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

mod assertions;
mod guards;
mod instanceof;
mod resolve;

pub(in crate::completion) use assertions::*;
pub(in crate::completion) use guards::*;
pub(in crate::completion) use instanceof::*;
pub(in crate::completion) use resolve::*;
