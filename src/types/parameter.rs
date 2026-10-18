//! A function or method parameter as parsed.

use super::*;

/// Stores extracted parameter information from a parsed PHP method.
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    /// The parameter name including the `$` prefix (e.g. "$text").
    pub name: Atom,
    /// Whether this parameter is required (no default value and not variadic).
    pub is_required: bool,
    /// Effective type hint after docblock override (e.g. `Collection<User>`).
    ///
    /// When a `@param` tag is present in the docblock and is more specific
    /// than the native PHP type hint, this holds the docblock type.
    /// Otherwise it holds the native type hint unchanged.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub type_hint: Option<PhpType>,
    /// The native PHP type hint as a parsed `PhpType` (e.g. `array`, `string`).
    ///
    /// Preserved separately so that hover can show the actual PHP declaration
    /// in the code block while displaying the richer docblock type alongside
    /// the FQN header.  `None` when no type hint is present in source.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub native_type_hint: Option<PhpType>,
    /// Human-readable description extracted from the `@param` tag.
    ///
    /// For `@param list<User> $users The active users`, this would be
    /// `Some("The active users")`.  `None` when no description text
    /// follows the parameter name in the `@param` tag.
    pub description: Option<String>,
    /// The source text of the default value expression (e.g. `"0"`, `"null"`,
    /// `"[]"`, `"'hello'"`).
    ///
    /// Extracted from the AST span when the parameter has a default value.
    /// `None` when the parameter has no default.
    pub default_value: Option<String>,
    /// Whether this parameter is variadic (has `...`).
    pub is_variadic: bool,
    /// Whether this parameter is passed by reference (has `&`).
    pub is_reference: bool,
    /// The type that `$this` resolves to inside a closure passed for this
    /// parameter, declared via the `@param-closure-this` PHPDoc tag.
    ///
    /// For example, `@param-closure-this \Illuminate\Routing\Route $callback`
    /// means that inside the closure passed as `$callback`, `$this` refers to
    /// `\Illuminate\Routing\Route` rather than the lexically enclosing class.
    /// Common in Laravel where closures are rebound via `Closure::bindTo()`.
    pub closure_this_type: Option<PhpType>,
    /// The type a by-reference parameter holds after the call returns,
    /// declared via the `@param-out` PHPDoc tag.
    ///
    /// Distinct from `type_hint`, which is what the caller may hand in.
    /// `@param-out` may name a type the declared input type doesn't
    /// admit at all (`@param A|null $arg` paired with `@param-out
    /// ($arg is null ? A&I : A) $arg`), so the two are tracked
    /// separately rather than merged. Read this through
    /// [`Self::out_type`], which falls back to the plain declared type
    /// when no `@param-out` tag is present.
    pub param_out_type: Option<PhpType>,
    /// When the callee runs a callable passed for this parameter, declared
    /// via `@param-immediately-invoked-callable` or
    /// `@param-later-invoked-callable`.
    ///
    /// `None` when neither tag is present. A call site then falls back to
    /// PHPStan's defaults: a function runs its callable arguments before
    /// it returns, a method keeps them for later.
    pub callable_invocation: Option<CallableInvocation>,
}

/// When a callee runs a callable it is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallableInvocation {
    /// Before the call returns, so what the callable writes through a
    /// by-reference capture is there once the call is done.
    Immediate,
    /// Some time later, any number of times, or never.
    Later,
}

impl ParameterInfo {
    /// Compare two parameters by signature-relevant fields only.
    ///
    /// Ignores `name_offset` (not present on this struct) and
    /// `description` (display-only).  Everything else affects type
    /// resolution and must trigger cache eviction when it changes.
    pub fn signature_eq(&self, other: &ParameterInfo) -> bool {
        self.name == other.name
            && self.is_required == other.is_required
            && self.type_hint == other.type_hint
            && self.default_value == other.default_value
            && self.is_variadic == other.is_variadic
            && self.is_reference == other.is_reference
            && self.closure_this_type == other.closure_this_type
            && self.param_out_type == other.param_out_type
            && self.callable_invocation == other.callable_invocation
    }

    /// Fold `null` into the effective type when the default value is the
    /// literal `null`.
    ///
    /// A parameter such as `Type $x = null` accepts null at runtime (the
    /// pre-8.4 implicit-nullable form), so its effective type must admit
    /// null. Call this after a docblock `@param` merge has (re)computed
    /// `type_hint`, since the merge would otherwise drop the implied null.
    /// The operation is idempotent.
    ///
    /// A type the default can bind a template in is left alone:
    /// `@param T $t` with `$t = null` is still `T`, and the `null` is one
    /// of the values `T` stands for rather than something added to it.
    /// `is_template` names the templates in scope.
    pub fn apply_null_default(&mut self, is_template: impl Fn(&str) -> bool) {
        if !self.defaults_to_null() {
            return;
        }
        let Some(t) = self.type_hint.take() else {
            return;
        };
        let names_template =
            |ty: &PhpType| matches!(ty.kind(), TypeKind::Named(n) if is_template(n));
        let binds_template = match t.kind() {
            TypeKind::Union(members) => members.iter().any(names_template),
            _ => names_template(&t),
        };
        self.type_hint = Some(if binds_template { t } else { t.or_null() });
    }

    /// Whether the declared default value is the literal `null`.
    pub fn defaults_to_null(&self) -> bool {
        self.default_value
            .as_deref()
            .is_some_and(|d| d.eq_ignore_ascii_case("null"))
    }

    /// The type the caller's argument holds *after* the call returns.
    ///
    /// For everything but a by-reference parameter this is just the
    /// declared type. A by-reference parameter that defaults to `null`
    /// is an out-parameter, and its null is the inverse of the one
    /// [`apply_null_default`](Self::apply_null_default) adds: it says the
    /// caller may leave the argument unset, not that the callee may leave
    /// it null. `preg_match(string $p, string $s, ?array &$m = null)`
    /// writes `$m` whenever it is passed, so reading an offset off it
    /// after the call is not a null dereference.
    ///
    /// Returns `None` when the parameter carries no type at all, and
    /// leaves a hint that is *only* `null` alone rather than erasing it.
    ///
    /// An explicit `@param-out` tag is authoritative and returned as-is,
    /// possibly a [`TypeKind::Conditional`](crate::php_type::TypeKind::Conditional)
    /// keyed on the parameter's own pre-call type — evaluating it against
    /// the call site is the caller's job, not this method's, since only
    /// the caller knows what was actually passed.
    pub fn out_type(&self) -> Option<PhpType> {
        if let Some(ref param_out) = self.param_out_type {
            return Some(param_out.clone());
        }
        let hint = self.type_hint.as_ref()?;
        if !self.is_reference || !self.defaults_to_null() {
            return Some(hint.clone());
        }
        Some(hint.non_null_type().unwrap_or_else(|| hint.clone()))
    }

    /// Return the type hint as a string, if present.
    ///
    /// Convenience wrapper around `self.type_hint.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn type_hint_str(&self) -> Option<String> {
        self.type_hint.as_ref().map(|t| t.to_string())
    }

    /// Return the native type hint as a string, if present.
    ///
    /// Convenience wrapper around `self.native_type_hint.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn native_type_hint_str(&self) -> Option<String> {
        self.native_type_hint.as_ref().map(|t| t.to_string())
    }
}
