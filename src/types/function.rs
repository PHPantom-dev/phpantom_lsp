//! A standalone function as parsed, and what a callable expression
//! resolves to.

use super::*;

/// Callback that resolves a function name to its [`FunctionInfo`].
///
/// Used by docblock generation and throws analysis to look up cross-file
/// function metadata.
pub type FunctionLoader<'a> = Option<&'a dyn Fn(&str, u32) -> Option<FunctionInfo>>;

/// The result of resolving a call expression to its callable target.
///
/// Shared between signature help (`resolve_callable`) and named-argument
/// completion (`resolve_named_arg_params`).  Each caller projects the
/// fields it needs from the result.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolvedCallableTarget {
    /// The parameters of the callable.
    ///
    /// A [`SharedVec`] so that projecting a method's parameters into a
    /// target (and caching the target per call site) shares the list
    /// instead of copying it.
    pub parameters: SharedVec<ParameterInfo>,
    /// Optional return type.
    pub return_type: Option<PhpType>,
    /// Whether the callable accepts any number of arguments without error,
    /// regardless of `parameters`. Set for a class with no explicit
    /// constructor: PHP silently ignores arguments to `new Foo(...)`, so
    /// the argument-count diagnostic must not flag extra arguments, while
    /// signature help still shows the (empty) signature.
    pub accepts_any_args: bool,
    /// Whether the body calls `func_get_args()` (or a sibling), so the
    /// callable accepts arguments beyond its declared parameters.
    pub reads_func_args: bool,
    /// Whether discarding the callable's result is reported, because it
    /// carries `#[\NoDiscard]`.
    pub no_discard: bool,
    /// Alternate parameter lists from overloaded function declarations.
    ///
    /// Populated from `FunctionInfo::overloads` when resolving standalone
    /// function calls.  The type checker tries each overload and only
    /// emits a diagnostic when the call is incompatible with ALL.
    pub overloads: Vec<Vec<ParameterInfo>>,
    /// FQN of the class the callable was resolved on, when it is a class
    /// member (`None` for standalone functions and closures).
    ///
    /// Consumers use it to substitute `self`/`static`/`parent` in the
    /// signature.  The syntactic prefix of the call expression cannot
    /// stand in for it: in `$this->state->canChangeTo(…)` the enclosing
    /// class is not the class that declares `canChangeTo`.
    pub owner_class: Option<Atom>,
    /// Parameter names (`$`-prefixed) whose substituted type came from
    /// resolving that same parameter's own argument, and only that
    /// argument (the `@template` it declares has no other binding site).
    ///
    /// Checking such an argument against its own substituted parameter
    /// type is circular: the substitution was produced by resolving this
    /// exact argument expression, so comparing the argument to it again
    /// through a possibly different resolution path can disagree and
    /// produce a false positive, never a genuine mismatch (e.g.
    /// PHPUnit's `assertSame(ExpectedType $expected, mixed $actual)`).
    /// The argument-compatibility diagnostic checks these parameters
    /// against the mapped type instead: the declared type with those
    /// templates replaced by their `of` bounds, or nothing at all (`None`)
    /// when none of them declares a bound.
    pub self_bound_params: crate::atom::AtomMap<Option<PhpType>>,
}

/// Stores extracted information about a standalone PHP function.
///
/// This is used for global / namespaced functions defined outside of classes,
/// typically found in files listed by Composer's `autoload_files.php`.
#[derive(Debug, Clone, Default)]
pub struct FunctionInfo {
    /// The function name (e.g. "array_map", "myHelper").
    pub name: Atom,
    /// Byte offset of the function's name token in the source file.
    ///
    /// Set to the `span.start.offset` of the name `LocalIdentifier` during
    /// parsing.  A value of `0` means "not available" (e.g. for stubs and
    /// synthetic entries) — callers should fall back to text search.
    pub name_offset: u32,
    /// The parameters of the function.
    ///
    /// A [`SharedVec`] for the same reason as
    /// [`MethodInfo::parameters`]: synthesized `FunctionInfo` values
    /// built from methods (callable targets, first-class callables)
    /// share the parameter list instead of copying it.
    pub parameters: SharedVec<ParameterInfo>,
    /// Effective return type after docblock override (e.g. `Collection<User>`).
    ///
    /// When a `@return` tag is present in the docblock and is more specific
    /// than the native PHP return type hint, this holds the docblock type.
    /// Otherwise it holds the native type hint unchanged.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub return_type: Option<PhpType>,
    /// The native PHP return type hint as a parsed `PhpType` (e.g. `array`, `self`).
    ///
    /// Preserved separately so that hover can show the actual PHP declaration
    /// in the code block while displaying the richer docblock type alongside
    /// the FQN header.  `None` when no return type hint is present in source.
    ///
    /// Call `.to_string()` when a display string is needed.
    pub native_return_type: Option<PhpType>,
    /// Human-readable description extracted from the function's docblock.
    ///
    /// This is the free-text portion of the docblock (before any `@tag` lines).
    /// `None` when the docblock has no description or no docblock is present.
    pub description: Option<String>,
    /// Human-readable description extracted from the `@return` tag.
    ///
    /// For `@return list<User> The active users`, this would be
    /// `Some("The active users")`.  `None` when no description text
    /// follows the type in the `@return` tag.
    pub return_description: Option<String>,
    /// URLs from `@link` and `@see` tags in the docblock.
    ///
    /// For `@link https://php.net/...` and `@see https://example.com/`,
    /// this collects all URLs found. Empty when no link/see URL tags are present.
    pub links: Vec<String>,
    /// Symbol and URL references from `@see` tags in the docblock.
    ///
    /// Each entry's `text` is what follows `@see`, which may be a symbol
    /// reference (e.g. `"UnsetDemo"`, `"MyClass::method()"`) or a URL
    /// (e.g. `"https://example.com/docs"`).  Rendered in hover below
    /// `@link` entries.
    pub see_refs: Vec<SeeRef>,
    /// The namespace this function is declared in, if any.
    /// For example, `Amp\delay` would have namespace `Some("Amp")`.
    pub namespace: Option<String>,
    /// Optional PHPStan conditional return type parsed from the docblock.
    ///
    /// When present, the resolver should use this instead of `return_type`
    /// and resolve the concrete type based on call-site arguments.
    ///
    /// Example docblock:
    /// ```text
    /// @return ($abstract is class-string<TClass> ? TClass : \Illuminate\Foundation\Application)
    /// ```
    pub conditional_return: Option<PhpType>,
    /// Type assertions parsed from `@phpstan-assert` / `@psalm-assert`
    /// annotations in the function's docblock.
    ///
    /// These allow user-defined functions to act as custom type guards,
    /// narrowing the type of a parameter after the call (or conditionally
    /// when used in an `if` condition).
    ///
    /// Example docblocks:
    /// ```text
    /// @phpstan-assert User $value           — unconditional assertion
    /// @phpstan-assert !User $value          — negated assertion
    /// @phpstan-assert-if-true User $value   — assertion when return is true
    /// @phpstan-assert-if-false User $value  — assertion when return is false
    /// ```
    pub type_assertions: Vec<TypeAssertion>,
    /// Deprecation message from the `@deprecated` PHPDoc tag.
    ///
    /// `None` means not deprecated. `Some("")` means deprecated without a
    /// message. `Some("Use newHelper() instead")` includes the explanation.
    pub deprecation_message: Option<String>,
    /// Replacement code template (from `#[Deprecated(replacement: "...")]`).
    ///
    /// Contains template variables like `%parametersList%`, `%parameter0%`,
    /// `%class%` that are expanded at call sites to offer a "replace
    /// deprecated call" code action.  `None` when no replacement is specified.
    pub deprecated_replacement: Option<String>,
    /// Template parameter names declared via `@template` tags in the
    /// function-level docblock.
    ///
    /// For example, a function with `@template T of Model` would have
    /// `template_params: vec!["T".into()]`.
    ///
    /// These mirror the `MethodInfo::template_params` field and are used
    /// for generic type substitution at call sites.
    pub template_params: Vec<Atom>,
    /// Mappings from function-level template parameter names to the
    /// function parameter names (with `$` prefix) that directly bind
    /// them via `@param` annotations.
    ///
    /// For example, `@template T` + `@param T $model` produces
    /// `[("T", "$model")]`.  At call sites the resolver uses these
    /// bindings to infer concrete types for each template parameter
    /// from the actual argument expressions.
    pub template_bindings: Vec<(Atom, Atom)>,
    /// Upper bounds for function-level template parameters
    /// (`@template T of Foo` → maps `"T"` to `Foo`).
    ///
    /// Used by `build_function_template_subs` to replace unbound
    /// template parameters with their declared bound (or `mixed`
    /// when no bound exists) so that raw template names never leak
    /// into downstream consumers.
    pub template_param_bounds: AtomMap<PhpType>,
    /// Defaults for function-level template parameters
    /// (`@template T = string`), which a call that leaves the parameter
    /// unbound resolves it to instead of the bound.
    pub template_param_defaults: Box<[(Atom, PhpType)]>,
    /// Exception types from `@throws` docblock tags.
    ///
    /// Populated during parsing from the function's docblock.  Used by
    /// the cross-file throws analysis to propagate exceptions from
    /// standalone function calls.
    pub throws: Vec<PhpType>,
    /// Whether this function was extracted from inside a
    /// `if (! function_exists('name'))` guard.
    ///
    /// Such functions are polyfills for native PHP functions introduced
    /// in newer versions.  When the configured PHP version already
    /// provides the native function (i.e. a stub exists in
    /// `stub_function_index`), the polyfill is dead code and should
    /// not shadow the stub's signature, deprecation status, or other
    /// metadata.
    pub is_polyfill: bool,
    /// Alternate parameter lists from overloaded function declarations.
    ///
    /// Some PHP functions (e.g. `strtr`, `implode`, `array_keys`) have
    /// multiple valid signatures that differ in parameter count and types.
    /// Stubs represent these as separate `function` declarations with the
    /// same name.  During parsing, duplicates are merged into this field.
    ///
    /// The type checker tries the primary `parameters` first, then each
    /// overload.  A diagnostic is only emitted when the call is
    /// incompatible with ALL signatures.
    pub overloads: Vec<Vec<ParameterInfo>>,
    /// Whether the function is declared side-effect free via `@pure`,
    /// `@phpstan-pure` or `@psalm-pure`.  See [`MethodInfo::is_pure`].
    pub is_pure: bool,
    /// Whether the function is declared to have side effects via
    /// `@impure`, `@phpstan-impure` or `@psalm-impure`.  See
    /// [`MethodInfo::is_impure`].
    pub is_impure: bool,
    /// Whether the body calls `func_get_args()` (or a sibling), so the
    /// callable accepts arguments beyond its declared parameters.
    pub reads_func_args: bool,
    /// Whether the function carries `#[\NoDiscard]`, so a call that throws
    /// its result away is reported.
    pub is_no_discard: bool,
    /// The types the docblock settled in place of a native hint while
    /// naming a class the hint does not, which loading the function keeps
    /// only where they narrow the hint.
    pub unverified_doc_types: Box<[UnverifiedDocType]>,
}

impl FunctionInfo {
    /// Return the return type as a string, if present.
    ///
    /// Convenience wrapper around `self.return_type.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn return_type_str(&self) -> Option<String> {
        self.return_type.as_ref().map(|t| t.to_string())
    }

    /// Return the native return type as a string, if present.
    ///
    /// Convenience wrapper around `self.native_return_type.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn native_return_type_str(&self) -> Option<String> {
        self.native_return_type.as_ref().map(|t| t.to_string())
    }

    /// Compare two `FunctionInfo` values for signature equality.
    ///
    /// Returns `true` when the public-facing signature (name, parameters,
    /// return types, template params, deprecation, throws) is identical.
    /// Fields that don't affect callers (description, links, name_offset)
    /// are intentionally excluded.
    ///
    /// Used by `update_ast` to detect cross-file invalidation: when a
    /// standalone function's signature changes, all open files that may
    /// call it need fresh diagnostics.
    pub fn signature_eq(&self, other: &FunctionInfo) -> bool {
        if self.name != other.name
            || self.return_type != other.return_type
            || self.native_return_type != other.native_return_type
            || self.conditional_return != other.conditional_return
            || self.deprecation_message != other.deprecation_message
            || self.deprecated_replacement != other.deprecated_replacement
            || self.template_params != other.template_params
            || self.template_bindings != other.template_bindings
            || self.template_param_bounds != other.template_param_bounds
            || self.template_param_defaults != other.template_param_defaults
            || self.type_assertions != other.type_assertions
            || self.throws != other.throws
            || self.namespace != other.namespace
            || self.unverified_doc_types != other.unverified_doc_types
        {
            return false;
        }

        if self.parameters.len() != other.parameters.len() {
            return false;
        }
        for (a, b) in self.parameters.iter().zip(other.parameters.iter()) {
            if !a.signature_eq(b) {
                return false;
            }
        }

        if self.overloads.len() != other.overloads.len() {
            return false;
        }

        true
    }
}
