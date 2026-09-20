//! Keyword and builtin-type name classifiers.

/// Whether a lowercased type name is a pseudo-type that only counts as one
/// when it is written in lowercase.
///
/// PHP's reserved type keywords are case-insensitive, so `FLOAT` is the scalar
/// and a class can never be named `Float`. The names here are different: none
/// of them is one of PHP's actual reserved type keywords (those are `int`,
/// `float`, `bool`, and `string`; PHP has no `resource` type-hint at all), so
/// a class can legally be named `Integer`, `Boolean`, `Double`, `Resource`,
/// `Number` (PHP 8.4's `BcMath\Number`), `Real`, `Scalar` (nikic/php-parser's
/// `PhpParser\Node\Scalar`), or `Numeric`. `integer`, `boolean`, and `double`
/// are only aliases `gettype()`/legacy PHPDoc use for `int`/`bool`/`float`;
/// `number` (`int|float`), `scalar` (`int|float|string|bool`) and `numeric`
/// (`int|float|numeric-string`) are PHPDoc-only pseudo-types; `real` was
/// removed from PHP in 8.0. Any casing other than all-lowercase is a class
/// reference and must not be folded into the scalar/alias.
pub(crate) fn is_lowercase_only_pseudo_type(lower: &str) -> bool {
    matches!(
        lower,
        "number" | "real" | "integer" | "boolean" | "double" | "resource" | "scalar" | "numeric"
    )
}

/// Lowercase `name` for keyword matching, leaving the casing of a
/// [`is_lowercase_only_pseudo_type`] name intact so a class of that name falls
/// through to the caller's class handling instead of matching the pseudo-type.
pub(crate) fn keyword_lowercase(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower != name && is_lowercase_only_pseudo_type(&lower) {
        return name.to_string();
    }
    lower
}

/// Whether `name` is a type PHP itself accepts in a native declaration.
///
/// This is PHP's own list, not the type vocabulary a docblock may draw on:
/// `resource`, `integer`, `number`, `list`, and every hyphenated PHPStan
/// spelling are missing from it deliberately. PHP reads an identifier it
/// does not recognise here as a class name ("`resource` is not a supported
/// builtin type and will be interpreted as a class name"), so anything
/// this returns `false` for is a class reference wherever a *native* type
/// is written, however well a docblock would understand it.
///
/// Native type names are reserved keywords, so they match in any casing.
pub(crate) fn is_native_type_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "int"
            | "float"
            | "string"
            | "bool"
            | "array"
            | "object"
            | "callable"
            | "iterable"
            | "mixed"
            | "void"
            | "never"
            | "null"
            | "false"
            | "true"
            | "self"
            | "static"
            | "parent"
    )
}

/// Whether a type name is a keyword that should never be resolved as a
/// class name.
///
/// This is a superset of [`is_scalar_name`] that also includes PHPDoc-only
/// pseudo-types and special names that should never be treated as class
/// references.
pub(crate) fn is_keyword_type(name: &str) -> bool {
    if is_scalar_name(name) {
        return true;
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        // ── Integer refinements ─────────────────────────────────
        "non-zero-int"
            | "int-mask"
            | "int-mask-of"
            // ── String refinements ──────────────────────────────────
            | "literal-string"
            | "callable-string"
            | "uppercase-string"
            | "non-empty-uppercase-string"
            | "non-empty-literal-string"
            // ── Class-string variants ───────────────────────────────
            | "trait-string"
            | "enum-string"
            // ── Array / list refinements ────────────────────────────
            | "associative-array"
            // ── Scalar / mixed variants ─────────────────────────────
            | "mixed"
            | "empty-scalar"
            | "non-empty-scalar"
            | "non-empty-mixed"
            | "empty"
            // ── Object / callable variants ──────────────────────────
            | "callable-object"
            | "callable-array"
            // ── Resource variants ───────────────────────────────────
            | "closed-resource"
            | "open-resource"
            // ── Never aliases ───────────────────────────────────────
            | "no-return"
            | "noreturn"
            | "never-return"
            | "never-returns"
            // ── Key / value projection ──────────────────────────────
            | "key-of"
            | "value-of"
            // ── Special keywords ────────────────────────────────────
            | "class"
            // ── PHPStan lenient-union wrapper ───────────────────────
            | "__benevolent"
    )
}

/// Whether `name` is a hyphenated PHPDoc pseudo-type spelling that no keyword
/// in [`is_keyword_type`] covers.
///
/// PHP identifiers cannot contain `-`, so such a name can never name a class:
/// it is a pseudo-type from some analyzer's vocabulary (`pure-callable`,
/// `literal-int`, `stringable-object`, …) that we have no model for. Resolving
/// it the way a class name is resolved qualifies it against the enclosing
/// namespace and then enforces a type no value can ever satisfy, so callers
/// degrade it to `mixed` instead.
pub(crate) fn is_unmodelled_pseudo_type(name: &str) -> bool {
    name.contains('-') && !is_keyword_type(name)
}

/// The type a PHPDoc refinement we have no model for widens to.
///
/// These spellings are recognized (the PHPDoc grammar has a keyword for each),
/// but the property each one adds — that an int is known at analysis time, that
/// a callable has no side effects, that an object has `__toString` — is not one
/// we can decide.  What we can decide is the type being refined, and every
/// value satisfying the refinement satisfies that, so widening keeps all the
/// accuracy available without enforcing a constraint we cannot check.
///
/// Matched in lowercase only, like the other PHPDoc-only pseudo-types: these
/// have no native spelling and any other casing is a class reference.
pub(crate) fn unmodelled_refinement_base(name: &str) -> Option<&'static str> {
    Some(match name {
        "literal-int" => "int",
        "literal-float" => "float",
        "lowercase-callable-string" | "uppercase-callable-string" => "callable-string",
        "stringable-object" => "object",
        "pure-callable" => "callable",
        // Fully qualified: `Closure` is a class, and a bare one would be
        // resolved against the docblock's own namespace.
        "pure-closure" => "\\Closure",
        _ => return None,
    })
}

/// Whether a type name is a primitive scalar / built-in type: `int`,
/// `float`, `string`, `bool`, `void`, `never`, `null`, `false`, `true`,
/// `array`, `callable`, `iterable`, `resource` (and PHP aliases).
///
/// Unlike [`is_scalar_name`], this excludes `mixed`, `object`, `self`,
/// `static`, `parent`, `class-string`, and other PHPDoc pseudo-types.
pub(crate) fn is_primitive_scalar_name(name: &str) -> bool {
    matches!(
        keyword_lowercase(name).as_str(),
        "int"
            | "integer"
            | "float"
            | "double"
            | "real"
            | "string"
            | "bool"
            | "boolean"
            | "void"
            | "never"
            | "null"
            | "false"
            | "true"
            | "array"
            | "callable"
            | "iterable"
            | "resource"
    )
}

/// Returns `true` when `name` is a built-in type keyword (case-sensitive) that
/// can never be a class name.  Covers PHP scalar/pseudo types and PHPStan
/// refinement types.
///
/// Only matches exact lowercase forms.  PHP allows classes named `Resource`,
/// `String`, `Object`, etc. (capitalised), so a case-insensitive check would
/// produce false positives.
///
/// Used by class resolution to short-circuit lookups for namespace-qualified
/// type hints like `Tests\Feature\int`, and by type resolution to skip class
/// loading for names that are obviously not classes.
pub(crate) fn is_builtin_non_class_type(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "float"
            | "string"
            | "bool"
            | "array"
            | "object"
            | "null"
            | "void"
            | "never"
            | "mixed"
            | "true"
            | "false"
            | "callable"
            | "iterable"
            | "resource"
            | "numeric"
            | "scalar"
            | "positive-int"
            | "negative-int"
            | "non-negative-int"
            | "non-positive-int"
            | "non-zero-int"
            | "numeric-string"
            | "non-empty-string"
            | "non-falsy-string"
            | "truthy-string"
            | "literal-string"
            | "non-empty-literal-string"
            | "lowercase-string"
            | "non-empty-lowercase-string"
            | "uppercase-string"
            | "non-empty-uppercase-string"
            | "callable-string"
            | "class-string"
            | "interface-string"
            | "trait-string"
            | "enum-string"
            | "model-property"
            | "array-key"
            | "list"
            | "non-empty-list"
            | "non-empty-array"
            | "empty"
            | "no-return"
            | "never-return"
            | "never-returns"
            | "number"
            | "double"
            | "boolean"
            | "integer"
            | "real"
    )
}

/// Returns `true` for type names that represent array-like types in PHP.
pub fn is_array_like_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "array" | "list" | "non-empty-array" | "non-empty-list" | "iterable"
    )
}

/// Returns `true` for array-like type names that promise at least one
/// element.
pub fn is_non_empty_array_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "non-empty-array" | "non-empty-list"
    )
}

/// Returns `true` for array-like type names that guarantee sequential
/// integer keys starting at zero.
pub fn is_list_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "list" | "non-empty-list"
    )
}

/// Public wrapper around [`is_scalar_name`] for use by other modules
/// (e.g. type-guard narrowing in `narrowing.rs`).
pub fn is_scalar_name_pub(name: &str) -> bool {
    is_scalar_name(name)
}

pub(crate) fn is_scalar_name(name: &str) -> bool {
    // `number` is a PHPDoc-only pseudo-type (int|float) and only in its exact
    // lowercase spelling. PHP has no native `number` type and allows a class
    // named `Number` (e.g. PHP 8.4's `BcMath\Number`), so any other casing is
    // a real class reference and must not be classified as a scalar.
    if name == "number" {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    // `integer`, `boolean`, `double`, `resource`, `scalar`, and `numeric` are
    // PHP aliases/pseudo-types, not reserved keywords (see
    // `is_lowercase_only_pseudo_type`), so a project may legally declare a
    // class of one of these names — `PhpParser\Node\Scalar` is one such class.
    // Only the exact lowercase spelling counts as the alias.
    if matches!(
        lower.as_str(),
        "integer" | "boolean" | "double" | "resource" | "scalar" | "numeric"
    ) {
        return name == lower;
    }
    matches!(
        lower.as_str(),
        "int"
            | "float"
            | "string"
            | "bool"
            | "void"
            | "never"
            | "null"
            | "false"
            | "true"
            | "array"
            | "callable"
            | "iterable"
            | "object"
            | "self"
            | "static"
            | "parent"
            | "$this"
            | "class-string"
            | "interface-string"
            | "trait-string"
            | "enum-string"
            | "model-property"
            | "numeric-string"
            | "non-empty-string"
            | "non-empty-lowercase-string"
            | "lowercase-string"
            | "uppercase-string"
            | "non-empty-uppercase-string"
            | "truthy-string"
            | "non-falsy-string"
            | "array-key"
            | "scalar"
            | "numeric"
            | "positive-int"
            | "negative-int"
            | "non-positive-int"
            | "non-negative-int"
            | "non-zero-int"
            | "non-empty-array"
            | "non-empty-list"
            | "list"
            | "associative-array"
            | "callable-string"
            | "callable-array"
            | "callable-object"
            | "literal-string"
            | "non-empty-literal-string"
            | "open-resource"
            | "closed-resource"
    )
}

/// Returns `true` for names that look like user-defined class,
/// interface, or enum names (as opposed to scalar types, keywords,
/// or pseudo-types).
///
/// This is used as a positive-space guard in the `"object"` subtype
/// arm of [`is_named_subtype`]: only names that pass this check are
/// treated as class-like (and therefore subtypes of `object`).
///
/// Names in [`is_scalar_name`] are excluded (not class-like).  Any
/// remaining identifier that starts with a letter or `_` is assumed
/// to be a class name.  This means unknown pseudo-types that are NOT
/// in `is_scalar_name` fail **open** (treated as class-like).  When
/// new PHPStan pseudo-types are introduced, they must be added to
/// `is_scalar_name` to avoid being misclassified as class names.
pub(crate) fn is_class_like_name(name: &str) -> bool {
    // FQN with namespace separator — definitely a class.
    if name.contains('\\') {
        return true;
    }
    // Known scalar/keyword/pseudo-types are not class-like.
    if is_scalar_name(name) {
        return false;
    }
    // After filtering out all known scalars, keywords, and pseudo-types,
    // any remaining name that starts with a valid PHP identifier character
    // is treated as a class name. PHP allows lowercase class names
    // (e.g. finfo, simplexmlelement), so we don't require uppercase.
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

/// Normalize keyword type casing and PHP aliases to their canonical forms.
///
/// Unlike [`native_scalar_name`], this does **not** collapse PHPStan
/// refinement types (`non-empty-string`, `positive-int`, etc.) to their
/// base PHP types.  It only handles:
///
/// - PHP aliases: `integer` → `int`, `boolean` → `bool`, `double`/`real` → `float`
/// - Case normalization: `NULL` → `null`, `TRUE` → `true`, `aRray` → `array`
///
/// Class names and unrecognised identifiers pass through unchanged.
pub(crate) fn normalize_keyword_casing(name: &str) -> String {
    let lower = keyword_lowercase(name);
    match lower.as_str() {
        // PHP aliases that map to a different canonical name.
        "integer" => "int".to_string(),
        "boolean" => "bool".to_string(),
        "double" | "real" => "float".to_string(),
        "no-return" | "noreturn" | "never-return" | "never-returns" => "never".to_string(),
        // Known keywords — return the lowercased form.
        "int" | "float" | "string" | "bool" | "void" | "never" | "null" | "false" | "true"
        | "mixed" | "object" | "array" | "callable" | "iterable" | "resource" | "self"
        | "static" | "parent"
        // PHPStan refinement types — lowercase but do NOT collapse.
        | "positive-int" | "negative-int" | "non-positive-int" | "non-negative-int"
        | "non-zero-int"
        | "non-empty-string" | "numeric-string" | "class-string" | "interface-string"
        | "literal-string" | "callable-string" | "truthy-string" | "non-falsy-string"
        | "trait-string" | "enum-string" | "lowercase-string" | "uppercase-string"
        | "non-empty-lowercase-string" | "non-empty-uppercase-string"
        | "non-empty-literal-string"
        | "non-empty-array" | "non-empty-list" | "non-empty-mixed" | "associative-array"
        | "closed-resource" | "open-resource" | "callable-object" | "callable-array"
        | "stringable-object"
        | "array-key" | "scalar" | "numeric" => lower,
        // `number`, `real`, `integer`, `boolean`, `double`, and `resource`
        // are aliases/pseudo-types only in lowercase; fall through so a
        // same-named class keeps its casing and the lowercase spellings
        // stay as-is.
        _ => name.to_string(),
    }
}

/// Map a PHPStan/docblock type name to its native PHP equivalent.
///
/// Returns `Some("int")` for `positive-int`, `Some("string")` for
/// `class-string`, `Some("array")` for `list`, etc.  Returns `None`
/// for names that have no single native PHP type (`scalar`, `numeric`,
/// `array-key`, `number`).  Class names pass through unchanged.
pub(crate) fn native_scalar_name(name: &str) -> Option<&str> {
    let lower = keyword_lowercase(name);
    match lower.as_str() {
        // Direct native types.
        "int" | "integer" => Some("int"),
        "float" | "double" | "real" => Some("float"),
        "string" => Some("string"),
        "bool" | "boolean" => Some("bool"),
        "void" => Some("void"),
        "never" | "no-return" | "noreturn" | "never-return" | "never-returns" => Some("never"),
        "null" => Some("null"),
        "false" => Some("false"),
        "true" => Some("true"),
        "array" | "non-empty-array" | "list" | "non-empty-list" | "associative-array" => {
            Some("array")
        }
        "callable" | "callable-object" | "callable-array" => Some("callable"),
        "iterable" => Some("iterable"),
        "resource" | "closed-resource" | "open-resource" => Some("resource"),
        "mixed" | "non-empty-mixed" => Some("mixed"),
        "object" => Some("object"),
        "self" => Some("self"),
        "static" | "$this" => Some("static"),
        "parent" => Some("parent"),

        // PHPStan int refinements → int.
        "positive-int" | "negative-int" | "non-positive-int" | "non-negative-int"
        | "non-zero-int" => Some("int"),

        // PHPStan string refinements → string.
        "non-empty-string"
        | "numeric-string"
        | "class-string"
        | "interface-string"
        | "literal-string"
        | "callable-string"
        | "truthy-string"
        | "non-falsy-string"
        | "trait-string"
        | "enum-string"
        | "lowercase-string"
        | "uppercase-string"
        | "non-empty-lowercase-string"
        | "non-empty-uppercase-string"
        | "non-empty-literal-string" => Some("string"),

        // Types with no single native equivalent.
        "scalar" | "numeric" | "array-key" | "empty-scalar" | "non-empty-scalar" | "empty" => None,

        // `number` (int|float) has no single native equivalent, but only in
        // its lowercase pseudo-type spelling; a `Number` class passes through.
        "number" if name == "number" => None,

        // Anything else is a class name — pass it through.
        _ => Some(name),
    }
}
