//! PHPStan `@phpstan-assert` type assertions.

/// A type assertion annotation parsed from `@phpstan-assert` /
/// `@psalm-assert` (and their `-if-true` / `-if-false` variants).
///
/// These annotations let any function or method act as a custom type
/// guard, telling the analyser that a parameter has been narrowed to
/// a specific type after the call succeeds.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAssertion {
    /// When the assertion applies.
    pub kind: AssertionKind,
    /// The parameter name **with** the `$` prefix (e.g. `"$value"`).
    pub param_name: String,
    /// The asserted type (e.g. `User`, `AdminUser`).
    ///
    /// Parsed from the raw docblock text via `PhpType::parse()`.
    /// Call `.to_string()` when a display string is needed.
    pub asserted_type: crate::php_type::PhpType,
    /// Whether the assertion is negated (`!Type`), meaning the parameter
    /// is guaranteed to *not* be this type.
    pub negated: bool,
    /// Whether the assertion was written with the `=` modifier (`=Type`,
    /// `!=Type`), asserting value equality rather than a subtype
    /// relationship.
    ///
    /// Within its own branch an equality assertion narrows exactly as the
    /// subtype form does, so this only matters in the opposite branch: an
    /// `-if-true` / `-if-false` tag is a one-way implication for the
    /// equality form, so it may not be inverted the way the subtype form
    /// can.  `filled()` promises `!=null|''` when it returns true, which
    /// says nothing about a value it returns false for.
    pub is_equality: bool,
}

/// When a `@phpstan-assert` annotation takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertionKind {
    /// `@phpstan-assert` — unconditional: after the function returns
    /// (without throwing), the assertion holds for all subsequent code.
    Always,
    /// `@phpstan-assert-if-true` — the assertion holds when the function
    /// returns `true` (i.e. inside the `if` body).
    IfTrue,
    /// `@phpstan-assert-if-false` — the assertion holds when the function
    /// returns `false` (i.e. inside the `else` body, or the `if` body of
    /// a negated condition).
    IfFalse,
}
