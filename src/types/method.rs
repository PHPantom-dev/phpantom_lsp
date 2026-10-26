//! A method as parsed.

use super::*;

/// Stores extracted method information from a parsed PHP class.
#[derive(Debug, Clone, Default)]
pub struct MethodInfo {
    /// The method name (e.g. "updateText").
    pub name: Atom,
    /// Byte offset of the method's name token in the source file.
    ///
    /// Set to the `span.start.offset` of the name `LocalIdentifier` during
    /// parsing.  A value of `0` means "not available" (e.g. for stubs and
    /// synthetic members) — callers should fall back to text search.
    pub name_offset: u32,
    /// The parameters of the method.
    ///
    /// A [`SharedVec`] so that transformed copies of a method (template
    /// substitution, Builder forwarding, mixin injection) share the
    /// parameter list with their origin when no parameter type is
    /// rewritten — which is the common case: substitution usually only
    /// changes the return type.  Mutation goes through
    /// [`SharedVec::make_mut`] (copy-on-write).
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
    /// Human-readable description extracted from the method's docblock.
    ///
    /// This is the free-text portion of the docblock (before any `@tag` lines).
    /// `None` when the docblock has no description or no docblock is present.
    pub description: Option<String>,
    /// Whether the method is static.
    pub is_static: bool,
    /// Visibility of the method (public, protected, or private).
    pub visibility: Visibility,
    /// Whether this method has the `#[Scope]` attribute (Laravel 11+).
    ///
    /// Methods decorated with `#[\Illuminate\Database\Eloquent\Attributes\Scope]`
    /// are treated as Eloquent scope methods without needing the `scopeX`
    /// naming convention.  The method's own name is used directly as the
    /// public-facing scope name (e.g. `#[Scope] protected function active()`
    /// becomes `User::active()`).
    pub has_scope_attribute: bool,
    /// Whether this method is declared `abstract`.
    ///
    /// Abstract methods have no body (`MethodBody::Abstract`) and must be
    /// implemented by concrete subclasses.  Interface methods are
    /// implicitly abstract.  Used by the "Implement missing methods"
    /// code action to detect which inherited methods still need stubs.
    pub is_abstract: bool,
    /// Whether this method is declared `final`.
    ///
    /// A `final` method cannot be redeclared by a subclass, so override
    /// completion must not offer one as a candidate.  Note that `final` on
    /// a *trait* method only binds subclasses of the class using the trait:
    /// the using class itself may still declare its own version.
    pub is_final: bool,
    /// Whether this method is a virtual (synthesized) member.
    ///
    /// Virtual methods come from `@method` docblock tags, `@mixin` classes,
    /// or framework-specific providers (e.g. Laravel model scopes).  They
    /// have no real declaration in source code.
    ///
    /// Set to `true` by [`MethodInfo::virtual_method`] and by providers;
    /// set to `false` by the parser for real declared methods.
    pub is_virtual: bool,
    /// Whether this method originates from a `::macro()` registration.
    ///
    /// Used by hover to show a "macro" indicator instead of the generic
    /// "virtual" label, so the user can distinguish macros from `@method`
    /// or `@mixin` synthesized members.
    pub is_macro: bool,
    /// Whether the return type was inferred from closure body analysis
    /// rather than explicitly declared via a type hint or docblock.
    ///
    /// When `true`, hover appends "(inferred)" to the return type line
    /// so the user knows the type is best-effort, not authoritative.
    pub is_inferred_return: bool,
    /// Whether the method is declared side-effect free via `@pure`,
    /// `@phpstan-pure` or `@psalm-pure`.
    ///
    /// A call that changes state behind its receiver invalidates the
    /// checks the forward walker recorded about that receiver, and this is
    /// the author's promise that it does not.  See [`Self::is_impure`] for
    /// the other half of the signal.
    pub is_pure: bool,
    /// Whether the method is declared to have side effects via `@impure`,
    /// `@phpstan-impure` or `@psalm-impure`.
    ///
    /// Without either tag, whether a call changes state is inferred from
    /// its return type: a method that returns nothing was called for its
    /// effect, and one that returns a value is assumed to compute it.  The
    /// tag is how an author overrides that for the value-returning case
    /// (`$stmt->execute(): bool`), which the return type cannot express.
    pub is_impure: bool,
    /// Whether the body calls `func_get_args()` (or a sibling), so the
    /// callable accepts arguments beyond its declared parameters.
    pub reads_func_args: bool,
    /// Whether the method carries `#[\NoDiscard]`, so a call that throws
    /// its result away is reported.  PHP does not inherit the attribute:
    /// an override without it may be called for its effect alone.
    pub is_no_discard: bool,
    /// Docblock-derived data that most methods leave empty (`@return`
    /// descriptions, links, deprecation, method-level templates, `@throws`,
    /// assertions, conditional and `$this`-narrowing types).
    ///
    /// `None` whenever every field would be empty, which holds for about
    /// three methods in four: inline, these fields cost close to 300 bytes
    /// per method even when unset. Read through the accessors
    /// ([`Self::throws`], [`Self::template_params`], ...) and write through
    /// [`Self::extras_mut`] or [`MethodExtras::into_option`].
    pub extras: Option<Box<MethodExtras>>,
}

/// The rarely-set part of a [`MethodInfo`], boxed behind
/// [`MethodInfo::extras`] so that methods without any of it pay one
/// pointer instead of the inline fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MethodExtras {
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
    /// Optional PHPStan conditional return type parsed from the docblock.
    ///
    /// When present, the resolver should use this instead of `return_type`
    /// and resolve the concrete type based on call-site arguments.
    ///
    /// Example docblock:
    /// ```text
    /// @return ($abstract is class-string<TClass> ? TClass : mixed)
    /// ```
    pub conditional_return: Option<PhpType>,
    /// Deprecation message from the `@deprecated` PHPDoc tag.
    ///
    /// `None` means not deprecated. `Some("")` means deprecated without a
    /// message. `Some("Use foo() instead")` includes the explanation.
    pub deprecation_message: Option<String>,
    /// Replacement code template (from `#[Deprecated(replacement: "...")]`).
    ///
    /// Contains template variables like `%parametersList%`, `%parameter0%`,
    /// `%class%` that are expanded at call sites to offer a "replace
    /// deprecated call" code action.  `None` when no replacement is specified.
    pub deprecated_replacement: Option<String>,
    /// Whether the docblock carries `@not-deprecated`, which stops an
    /// ancestor's `@deprecated` from reaching this method and everything
    /// that inherits it.
    pub not_deprecated: bool,
    /// Template parameter names declared via `@template` tags in the
    /// method-level docblock.
    ///
    /// For example, a method with `@template T of Model` would have
    /// `template_params: vec!["T".into()]`.
    ///
    /// These are distinct from class-level template parameters
    /// (`ClassInfo::template_params`) and are used for general
    /// method-level generic type substitution at call sites.
    pub template_params: Vec<Atom>,
    /// Upper bounds for method-level template parameters.
    ///
    /// For `@template T of Model`, maps `"T"` → `PhpType::parse("Model")`.
    /// Used by hover to display the constraint when the return type or a
    /// parameter type is a method-level template parameter.
    pub template_param_bounds: AtomMap<PhpType>,
    /// Defaults for method-level template parameters
    /// (`@template T of object = \stdClass`), which a call that leaves the
    /// parameter unbound resolves it to instead of the bound.
    ///
    /// A slice rather than a map: most methods declare none, and an empty
    /// boxed slice costs half an empty map on every `MethodInfo`.
    pub template_param_defaults: Box<[(Atom, PhpType)]>,
    /// Mappings from method-level template parameter names to the method
    /// parameter names (with `$` prefix) that directly bind them via
    /// `@param` annotations.
    ///
    /// For example, `@template T` + `@param T $model` produces
    /// `[("T", "$model")]`.  At call sites the resolver uses these
    /// bindings to infer concrete types for each template parameter
    /// from the actual argument expressions.
    pub template_bindings: Vec<(Atom, Atom)>,
    /// Type assertions declared via `@phpstan-assert` / `@psalm-assert` tags
    /// in the method's docblock.
    ///
    /// Works identically to [`FunctionInfo::type_assertions`] but for class
    /// methods.  Used by the narrowing engine to apply type guards from
    /// static method calls like `Assert::instanceOf($value, Foo::class)`.
    pub type_assertions: Vec<TypeAssertion>,
    /// Exception types declared via `@throws` tags in the method's docblock.
    ///
    /// For example, a method with `@throws \InvalidArgumentException` would have
    /// `throws: vec![PhpType::named(atom("InvalidArgumentException"))]`.  Used
    /// during code generation and analysis to propagate exception information.
    pub throws: Vec<PhpType>,
    /// Type constraint from `@psalm-if-this-is` or `@phpstan-if-this-is`.
    ///
    /// When present, the method's return type should only be applied if
    /// the receiver's type matches this pattern. Template parameters in
    /// the pattern are resolved against the caller's concrete type to
    /// compute additional template substitutions for the return type.
    pub if_this_is: Option<PhpType>,
    /// Receiver type mutation from `@psalm-this-out` or `@phpstan-self-out`.
    ///
    /// When present, a call to this method changes the type the forward
    /// walker believes the receiver variable holds, the way an assignment
    /// changes a variable's type. Method-level template parameters bound
    /// from the call's arguments (and the receiver's own generic
    /// arguments) are substituted into this type before it replaces the
    /// receiver's tracked type.
    pub self_out: Option<PhpType>,
}

impl MethodExtras {
    /// Whether every field is empty, i.e. the method needs no extras box.
    pub fn is_empty(&self) -> bool {
        self.return_description.is_none()
            && self.links.is_empty()
            && self.see_refs.is_empty()
            && self.conditional_return.is_none()
            && self.deprecation_message.is_none()
            && self.deprecated_replacement.is_none()
            && !self.not_deprecated
            && self.template_params.is_empty()
            && self.template_param_bounds.is_empty()
            && self.template_param_defaults.is_empty()
            && self.template_bindings.is_empty()
            && self.type_assertions.is_empty()
            && self.throws.is_empty()
            && self.if_this_is.is_none()
            && self.self_out.is_none()
    }

    /// Box `self` for [`MethodInfo::extras`], or `None` when it is empty.
    pub fn into_option(self) -> Option<Box<Self>> {
        if self.is_empty() {
            None
        } else {
            Some(Box::new(self))
        }
    }
}

impl MethodInfo {
    /// Compare two methods by signature-relevant fields only.
    ///
    /// Ignores fields that change on every keystroke (byte offsets).
    /// Everything else — including descriptions and links — affects
    /// either type resolution or hover display and must trigger cache
    /// eviction when it changes.
    ///
    /// Parameters are compared in order (not as sets) because parameter
    /// order matters for signature help and call resolution.
    pub fn signature_eq(&self, other: &MethodInfo) -> bool {
        self.name == other.name
            && self.is_static == other.is_static
            && self.visibility == other.visibility
            && self.return_type == other.return_type
            && self.native_return_type == other.native_return_type
            && self.description == other.description
            && self.extras_eq(other)
            && self.has_scope_attribute == other.has_scope_attribute
            && self.is_abstract == other.is_abstract
            && self.is_final == other.is_final
            && self.is_virtual == other.is_virtual
            && self.is_macro == other.is_macro
            && self.is_inferred_return == other.is_inferred_return
            && self.is_pure == other.is_pure
            && self.is_impure == other.is_impure
            && self.parameters.len() == other.parameters.len()
            && self
                .parameters
                .iter()
                .zip(other.parameters.iter())
                .all(|(a, b)| a.signature_eq(b))
    }

    /// Whether both methods carry the same extras, treating an empty box
    /// as equal to none.
    pub fn extras_eq(&self, other: &MethodInfo) -> bool {
        match (self.extras.as_deref(), other.extras.as_deref()) {
            (Some(a), Some(b)) => a == b,
            (Some(e), None) | (None, Some(e)) => e.is_empty(),
            (None, None) => true,
        }
    }

    /// Mutable access to the extras, allocating them on first use.
    ///
    /// Only call this when about to store something: a box left empty
    /// costs an allocation that [`MethodExtras::into_option`] would have
    /// avoided.
    pub fn extras_mut(&mut self) -> &mut MethodExtras {
        self.extras.get_or_insert_with(Default::default)
    }

    /// The `@return` tag's description text.
    pub fn return_description(&self) -> Option<&str> {
        self.extras.as_ref()?.return_description.as_deref()
    }

    /// URLs from `@link` and `@see` tags.
    pub fn links(&self) -> &[String] {
        self.extras.as_ref().map_or(&[], |e| &e.links)
    }

    /// Raw `@see` references.
    pub fn see_refs(&self) -> &[SeeRef] {
        self.extras.as_ref().map_or(&[], |e| &e.see_refs)
    }

    /// The PHPStan conditional return type, if declared.
    pub fn conditional_return(&self) -> Option<&PhpType> {
        self.extras.as_ref()?.conditional_return.as_ref()
    }

    /// The `@deprecated` message (`Some("")` when deprecated without one).
    pub fn deprecation_message(&self) -> Option<&str> {
        self.extras.as_ref()?.deprecation_message.as_deref()
    }

    /// Whether the method is tagged `@not-deprecated`.
    pub fn not_deprecated(&self) -> bool {
        self.extras.as_ref().is_some_and(|e| e.not_deprecated)
    }

    /// The `#[Deprecated(replacement: ...)]` code template.
    pub fn deprecated_replacement(&self) -> Option<&str> {
        self.extras.as_ref()?.deprecated_replacement.as_deref()
    }

    /// Method-level `@template` parameter names.
    pub fn template_params(&self) -> &[Atom] {
        self.extras.as_ref().map_or(&[], |e| &e.template_params)
    }

    /// Upper bounds of the method-level template parameters.
    pub fn template_param_bounds(&self) -> &AtomMap<PhpType> {
        static EMPTY: AtomMap<PhpType> = HashMap::with_hasher(std::hash::BuildHasherDefault::new());
        self.extras
            .as_ref()
            .map_or(&EMPTY, |e| &e.template_param_bounds)
    }

    /// Defaults of the method-level template parameters.
    pub fn template_param_defaults(&self) -> &[(Atom, PhpType)] {
        self.extras
            .as_ref()
            .map_or(&[], |e| &e.template_param_defaults)
    }

    /// Template parameter to `$parameter` bindings from `@param` tags.
    pub fn template_bindings(&self) -> &[(Atom, Atom)] {
        self.extras.as_ref().map_or(&[], |e| &e.template_bindings)
    }

    /// `@phpstan-assert` / `@psalm-assert` type assertions.
    pub fn type_assertions(&self) -> &[TypeAssertion] {
        self.extras.as_ref().map_or(&[], |e| &e.type_assertions)
    }

    /// Exception types from `@throws` tags.
    pub fn throws(&self) -> &[PhpType] {
        self.extras.as_ref().map_or(&[], |e| &e.throws)
    }

    /// The `@phpstan-if-this-is` receiver constraint.
    pub fn if_this_is(&self) -> Option<&PhpType> {
        self.extras.as_ref()?.if_this_is.as_ref()
    }

    /// The `@phpstan-self-out` receiver mutation.
    pub fn self_out(&self) -> Option<&PhpType> {
        self.extras.as_ref()?.self_out.as_ref()
    }

    /// Return the return type as a string, if present.
    ///
    /// Convenience wrapper around `self.return_type.as_ref().map(|t| t.to_string())`.
    /// Use this when you need a display string (hover, completion detail,
    /// code generation).
    pub fn return_type_str(&self) -> Option<String> {
        self.return_type.as_ref().map(|t| t.to_string())
    }

    /// Create a virtual `MethodInfo` with sensible defaults.
    ///
    /// The method is public, non-static, non-deprecated, with no
    /// parameters, no template params, and `name_offset: 0`.
    ///
    /// Use struct update syntax to override individual fields:
    ///
    /// ```ignore
    /// MethodInfo {
    ///     is_static: true,
    ///     parameters: params,
    ///     ..MethodInfo::virtual_method("foo", Some("string"))
    /// }
    /// ```
    pub fn virtual_method(name: &str, return_type: Option<&str>) -> Self {
        Self::virtual_method_typed(name, return_type.map(PhpType::parse).as_ref())
    }

    /// Like [`virtual_method`], but accepts the return type as a
    /// `PhpType` directly, avoiding the `PhpType → String → PhpType`
    /// round-trip when the caller already holds a `PhpType`.
    pub fn virtual_method_typed(name: &str, return_type: Option<&PhpType>) -> Self {
        Self {
            name: crate::atom::atom(name),
            name_offset: 0,
            parameters: SharedVec::new(),
            return_type: return_type.cloned(),
            native_return_type: None,
            description: None,
            is_static: false,
            visibility: Visibility::Public,
            has_scope_attribute: false,
            is_abstract: false,
            is_final: false,
            is_virtual: true,
            is_macro: false,
            is_inferred_return: false,
            is_pure: false,
            is_impure: false,
            reads_func_args: false,
            is_no_discard: false,
            extras: None,
        }
    }
}
