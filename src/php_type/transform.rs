//! Type transformations: name resolution, self-substitution, generics.

use super::*;

impl PhpType {
    /// Produce a new `PhpType` with all class names resolved through
    /// the provided callback.
    ///
    /// The callback receives each class-like name (from `Named`,
    /// `Generic`, `ClassString`, etc.) and returns the resolved
    /// fully-qualified name. Names that are keywords/scalars are
    /// never passed to the callback.
    ///
    /// This replaces the character-by-character `resolve_type_string`
    /// function in `ast_update.rs` with a clean tree traversal.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let ty = PhpType::parse("Collection<int, User>|null");
    /// let resolved = ty.resolve_names(&|name| {
    ///     use_map.get(name).cloned()
    ///         .unwrap_or_else(|| format!("App\\{}", name))
    /// });
    /// // → Generic("App\\Collection", [Named("int"), Named("App\\User")]) | Named("null")
    /// ```
    pub fn resolve_names(&self, resolver: &dyn Fn(&str) -> String) -> PhpType {
        match self {
            PhpType::Named(s) => {
                if is_keyword_type(s) {
                    PhpType::Named(s.clone())
                } else {
                    PhpType::Named(resolver(s))
                }
            }

            PhpType::Nullable(inner) => PhpType::Nullable(Box::new(inner.resolve_names(resolver))),

            PhpType::Union(types) => {
                PhpType::Union(types.iter().map(|t| t.resolve_names(resolver)).collect())
            }

            PhpType::Intersection(types) => {
                PhpType::Intersection(types.iter().map(|t| t.resolve_names(resolver)).collect())
            }

            PhpType::Generic(name, args) => {
                let resolved_name = if is_keyword_type(name) {
                    name.clone()
                } else {
                    resolver(name)
                };
                PhpType::Generic(
                    resolved_name,
                    args.iter().map(|a| a.resolve_names(resolver)).collect(),
                )
            }

            PhpType::Array(inner) => PhpType::Array(Box::new(inner.resolve_names(resolver))),

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.resolve_names(resolver),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.resolve_names(resolver),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: if is_keyword_type(kind) {
                    kind.clone()
                } else {
                    resolver(kind)
                },
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.resolve_names(resolver),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type
                    .as_ref()
                    .map(|rt| Box::new(rt.resolve_names(resolver))),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.resolve_names(resolver)),
                then_type: Box::new(then_type.resolve_names(resolver)),
                else_type: Box::new(else_type.resolve_names(resolver)),
            },

            PhpType::ClassString(inner) => {
                PhpType::ClassString(inner.as_ref().map(|i| Box::new(i.resolve_names(resolver))))
            }

            PhpType::InterfaceString(inner) => PhpType::InterfaceString(
                inner.as_ref().map(|i| Box::new(i.resolve_names(resolver))),
            ),

            PhpType::KeyOf(inner) => PhpType::KeyOf(Box::new(inner.resolve_names(resolver))),

            PhpType::ValueOf(inner) => PhpType::ValueOf(Box::new(inner.resolve_names(resolver))),

            PhpType::IntRange(min, max) => PhpType::IntRange(min.clone(), max.clone()),

            PhpType::IndexAccess(target, index) => PhpType::IndexAccess(
                Box::new(target.resolve_names(resolver)),
                Box::new(index.resolve_names(resolver)),
            ),

            PhpType::Literal(s) => PhpType::Literal(s.clone()),

            // Raw types can't be structurally resolved — pass through.
            PhpType::Raw(s) => PhpType::Raw(s.clone()),
        }
    }

    /// Return the short (unqualified) name from a potentially
    /// namespace-qualified type name. Returns only the part after the
    /// last `\`. Non-class types pass through unchanged.
    pub(crate) fn short_name_of(name: &str) -> &str {
        crate::util::short_name(name.trim())
    }

    /// Produce a new `PhpType` with all namespace-qualified names
    /// shortened to their unqualified form.
    ///
    /// For example, `App\Models\User|null` becomes `User|null`, and
    /// `array<int, App\Models\User>` becomes `array<int, User>`.
    pub fn shorten(&self) -> PhpType {
        match self {
            PhpType::Named(s) => PhpType::Named(Self::short_name_of(s).to_owned()),

            PhpType::Nullable(inner) => PhpType::Nullable(Box::new(inner.shorten())),

            PhpType::Union(types) => PhpType::Union(types.iter().map(|t| t.shorten()).collect()),

            PhpType::Intersection(types) => {
                PhpType::Intersection(types.iter().map(|t| t.shorten()).collect())
            }

            PhpType::Generic(name, args) => PhpType::Generic(
                Self::short_name_of(name).to_owned(),
                args.iter().map(|a| a.shorten()).collect(),
            ),

            PhpType::Array(inner) => PhpType::Array(Box::new(inner.shorten())),

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.shorten(),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.shorten(),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: Self::short_name_of(kind).to_owned(),
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.shorten(),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type.as_ref().map(|rt| Box::new(rt.shorten())),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.shorten()),
                then_type: Box::new(then_type.shorten()),
                else_type: Box::new(else_type.shorten()),
            },

            PhpType::ClassString(inner) => {
                PhpType::ClassString(inner.as_ref().map(|i| Box::new(i.shorten())))
            }

            PhpType::InterfaceString(inner) => {
                PhpType::InterfaceString(inner.as_ref().map(|i| Box::new(i.shorten())))
            }

            PhpType::KeyOf(inner) => PhpType::KeyOf(Box::new(inner.shorten())),

            PhpType::ValueOf(inner) => PhpType::ValueOf(Box::new(inner.shorten())),

            PhpType::IntRange(min, max) => PhpType::IntRange(min.clone(), max.clone()),

            PhpType::IndexAccess(target, index) => {
                PhpType::IndexAccess(Box::new(target.shorten()), Box::new(index.shorten()))
            }

            PhpType::Literal(s) => PhpType::Literal(s.clone()),

            PhpType::Raw(s) => {
                // Best-effort: apply the old string-based shortening
                // for raw types that we couldn't parse structurally.
                PhpType::Raw(s.clone())
            }
        }
    }

    /// Produce a new `PhpType` with `self`, `static`, and `$this`
    /// replaced by the given class name.
    ///
    /// Walks the entire type tree and replaces any `Named("self")`,
    /// `Named("static")`, or `Named("$this")` with
    /// `Named(class_name)`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let ty = PhpType::parse("self|null");
    /// let replaced = ty.replace_self("App\\User");
    /// assert_eq!(replaced.to_string(), "App\\User | null");
    /// ```
    pub fn replace_self(&self, class_name: &str) -> PhpType {
        self.replace_self_with_type(&PhpType::Named(class_name.to_string()))
    }

    /// Resolve relative class-reference keywords to concrete class names,
    /// walking the entire type tree (including array elements and generic
    /// arguments).
    ///
    /// `self`, `static`, and `$this` become `class_name`; `parent` becomes
    /// `parent_class` when it is `Some`.  Unlike [`resolve_names`], which
    /// treats these keywords as non-class types and leaves them untouched,
    /// this resolves them so a declared type can be compared against a
    /// resolved value type.
    ///
    /// [`resolve_names`]: PhpType::resolve_names
    pub fn resolve_self_refs(&self, class_name: &str, parent_class: Option<&str>) -> PhpType {
        // self / static / $this — case-insensitive, whole-tree walk.
        let replaced = self.replace_self(class_name);
        match parent_class {
            Some(parent) => {
                let subs = std::collections::HashMap::from([(
                    "parent".to_string(),
                    PhpType::Named(parent.to_string()),
                )]);
                replaced.substitute(&subs)
            }
            None => replaced,
        }
    }

    /// Replace only the `self` keyword (not `static` or `$this`) with a
    /// concrete class name.  Used during inheritance merging so that
    /// inherited methods carry the declaring class's identity for `self`
    /// while preserving `static` for late-static-binding resolution.
    pub fn replace_bare_self(&self, class_name: &str) -> PhpType {
        match self {
            PhpType::Named(s) if s.eq_ignore_ascii_case("self") => {
                PhpType::Named(class_name.to_string())
            }
            PhpType::Named(_) | PhpType::Literal(_) | PhpType::Raw(_) => self.clone(),
            PhpType::Nullable(inner) => {
                PhpType::Nullable(Box::new(inner.replace_bare_self(class_name)))
            }
            PhpType::Union(types) => PhpType::Union(
                types
                    .iter()
                    .map(|t| t.replace_bare_self(class_name))
                    .collect(),
            ),
            PhpType::Intersection(types) => PhpType::Intersection(
                types
                    .iter()
                    .map(|t| t.replace_bare_self(class_name))
                    .collect(),
            ),
            PhpType::Generic(name, args) => {
                let resolved_name = if name.eq_ignore_ascii_case("self") {
                    class_name.to_string()
                } else {
                    name.clone()
                };
                PhpType::Generic(
                    resolved_name,
                    args.iter()
                        .map(|a| a.replace_bare_self(class_name))
                        .collect(),
                )
            }
            PhpType::Array(inner) => PhpType::Array(Box::new(inner.replace_bare_self(class_name))),
            _ => self.clone(),
        }
    }

    /// Returns `true` when this type contains the bare `self` keyword
    /// (not `static` or `$this`).
    pub fn contains_bare_self(&self) -> bool {
        match self {
            PhpType::Named(s) => s.eq_ignore_ascii_case("self"),
            PhpType::Nullable(inner) => inner.contains_bare_self(),
            PhpType::Union(types) | PhpType::Intersection(types) => {
                types.iter().any(|t| t.contains_bare_self())
            }
            PhpType::Generic(name, args) => {
                name.eq_ignore_ascii_case("self") || args.iter().any(|a| a.contains_bare_self())
            }
            PhpType::Array(inner) => inner.contains_bare_self(),
            _ => false,
        }
    }

    /// Check whether this type tree contains any `self`, `static`, or
    /// `$this` references that [`replace_self`] / [`replace_self_with_type`]
    /// would replace.
    pub fn contains_self_ref(&self) -> bool {
        match self {
            PhpType::Named(_) => self.is_self_ref(),
            PhpType::Nullable(inner) => inner.contains_self_ref(),
            PhpType::Union(types) | PhpType::Intersection(types) => {
                types.iter().any(|t| t.contains_self_ref())
            }
            PhpType::Generic(name, args) => {
                is_self_ref_name(name) || args.iter().any(|a| a.contains_self_ref())
            }
            PhpType::Array(inner) => inner.contains_self_ref(),
            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
                entries.iter().any(|e| e.value_type.contains_self_ref())
            }
            PhpType::Callable {
                params,
                return_type,
                ..
            } => {
                params.iter().any(|p| p.type_hint.contains_self_ref())
                    || return_type.as_ref().is_some_and(|r| r.contains_self_ref())
            }
            PhpType::Conditional {
                condition,
                then_type,
                else_type,
                ..
            } => {
                condition.contains_self_ref()
                    || then_type.contains_self_ref()
                    || else_type.contains_self_ref()
            }
            PhpType::ClassString(inner) | PhpType::InterfaceString(inner) => {
                inner.as_ref().is_some_and(|t| t.contains_self_ref())
            }
            PhpType::KeyOf(inner) | PhpType::ValueOf(inner) => inner.contains_self_ref(),
            PhpType::IndexAccess(base, index) => {
                base.contains_self_ref() || index.contains_self_ref()
            }
            PhpType::Literal(_) | PhpType::Raw(_) | PhpType::IntRange(_, _) => false,
        }
    }

    /// Replace `self` / `static` / `$this` throughout this type tree
    /// with the given [`PhpType`].
    ///
    /// This is the structured counterpart of [`replace_self`]: instead of
    /// replacing with a bare class name (`PhpType::Named(name)`), it
    /// substitutes a full type expression.  This preserves generic
    /// parameters when the receiver is a generic type like
    /// `Builder<Article>`.
    ///
    /// When `replacement` is `PhpType::Generic("Builder", [Named("Article")])`
    /// and the return type is `Named("static")`, the result is the full
    /// generic type.  When the return type is `Generic("static", [args])`,
    /// the replacement's base name is used and the return type's own args
    /// are kept (they override the receiver's args).
    pub fn replace_self_with_type(&self, replacement: &PhpType) -> PhpType {
        // Extract the base class name from the replacement for use in
        // Generic nodes where only the name part is replaced.
        let replacement_name = match replacement {
            PhpType::Named(n) => n.as_str(),
            PhpType::Generic(n, _) => n.as_str(),
            _ => "",
        };
        match self {
            PhpType::Named(_) if self.is_self_ref() => replacement.clone(),

            PhpType::Named(_) | PhpType::Literal(_) | PhpType::Raw(_) => self.clone(),

            PhpType::Nullable(inner) => {
                PhpType::Nullable(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::Union(types) => PhpType::Union(
                types
                    .iter()
                    .map(|t| t.replace_self_with_type(replacement))
                    .collect(),
            ),

            PhpType::Intersection(types) => PhpType::Intersection(
                types
                    .iter()
                    .map(|t| t.replace_self_with_type(replacement))
                    .collect(),
            ),

            PhpType::Generic(name, args) => {
                let resolved_name = if is_self_ref_name(name) {
                    replacement_name.to_string()
                } else {
                    name.clone()
                };
                PhpType::Generic(
                    resolved_name,
                    args.iter()
                        .map(|a| a.replace_self_with_type(replacement))
                        .collect(),
                )
            }

            PhpType::Array(inner) => {
                PhpType::Array(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.replace_self_with_type(replacement),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.replace_self_with_type(replacement),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: kind.clone(),
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.replace_self_with_type(replacement),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type
                    .as_ref()
                    .map(|r| Box::new(r.replace_self_with_type(replacement))),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.replace_self_with_type(replacement)),
                then_type: Box::new(then_type.replace_self_with_type(replacement)),
                else_type: Box::new(else_type.replace_self_with_type(replacement)),
            },

            PhpType::ClassString(inner) => PhpType::ClassString(
                inner
                    .as_ref()
                    .map(|t| Box::new(t.replace_self_with_type(replacement))),
            ),

            PhpType::InterfaceString(inner) => PhpType::InterfaceString(
                inner
                    .as_ref()
                    .map(|t| Box::new(t.replace_self_with_type(replacement))),
            ),

            PhpType::KeyOf(inner) => {
                PhpType::KeyOf(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::ValueOf(inner) => {
                PhpType::ValueOf(Box::new(inner.replace_self_with_type(replacement)))
            }

            PhpType::IntRange(lo, hi) => PhpType::IntRange(lo.clone(), hi.clone()),

            PhpType::IndexAccess(base, index) => PhpType::IndexAccess(
                Box::new(base.replace_self_with_type(replacement)),
                Box::new(index.replace_self_with_type(replacement)),
            ),
        }
    }

    /// Substitute template parameter names throughout this type tree.
    ///
    /// Walks the entire type tree and replaces any `Named(s)` node whose
    /// name appears as a key in `subs` with `PhpType::parse(replacement)`.
    /// All other nodes are recursively rebuilt with their children
    /// substituted.
    ///
    /// This is the structured-type equivalent of the string-surgery
    /// `apply_substitution` function in `inheritance.rs`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::collections::HashMap;
    /// let ty = PhpType::parse("Collection<TKey, TValue>");
    /// let subs: HashMap<String, PhpType> =
    ///     [("TKey".into(), PhpType::parse("int")), ("TValue".into(), PhpType::parse("User"))]
    ///         .into_iter().collect();
    /// let result = ty.substitute(&subs);
    /// assert_eq!(result.to_string(), "Collection<int, User>");
    /// ```
    pub fn substitute(&self, subs: &std::collections::HashMap<String, PhpType>) -> PhpType {
        if subs.is_empty() {
            return self.clone();
        }
        match self {
            PhpType::Named(s) => {
                if let Some(replacement) = subs.get(s.as_str()) {
                    replacement.clone()
                } else {
                    self.clone()
                }
            }

            PhpType::Literal(_) | PhpType::Raw(_) | PhpType::IntRange(_, _) => self.clone(),

            PhpType::Nullable(inner) => {
                let resolved = inner.substitute(subs);
                // If the substitution produced a union or nullable,
                // don't double-wrap.
                match &resolved {
                    PhpType::Nullable(_) => resolved,
                    PhpType::Union(members) => {
                        // Already nullable if it contains null
                        if members.iter().any(
                            |m| matches!(m, PhpType::Named(n) if n.eq_ignore_ascii_case("null")),
                        ) {
                            resolved
                        } else {
                            PhpType::Nullable(Box::new(resolved))
                        }
                    }
                    _ => PhpType::Nullable(Box::new(resolved)),
                }
            }

            PhpType::Union(types) => {
                let resolved: Vec<PhpType> = types.iter().map(|t| t.substitute(subs)).collect();
                // Flatten any nested unions produced by substitution.
                let mut flat = Vec::with_capacity(resolved.len());
                for t in resolved {
                    match t {
                        PhpType::Union(inner) => flat.extend(inner),
                        other => flat.push(other),
                    }
                }
                if flat.len() == 1 {
                    flat.into_iter().next().unwrap()
                } else {
                    PhpType::Union(flat)
                }
            }

            PhpType::Intersection(types) => {
                let resolved: Vec<PhpType> = types.iter().map(|t| t.substitute(subs)).collect();
                let mut flat = Vec::with_capacity(resolved.len());
                for t in resolved {
                    match t {
                        PhpType::Intersection(inner) => flat.extend(inner),
                        other => flat.push(other),
                    }
                }
                if flat.len() == 1 {
                    flat.into_iter().next().unwrap()
                } else {
                    PhpType::Intersection(flat)
                }
            }

            PhpType::Generic(name, args) => {
                // The base name might itself be a template parameter.
                if let Some(replacement) = subs.get(name.as_str()) {
                    match replacement {
                        PhpType::Named(n) => PhpType::Generic(
                            n.clone(),
                            args.iter().map(|a| a.substitute(subs)).collect(),
                        ),
                        PhpType::Generic(base, _) => {
                            // Use the replacement's base name but keep the
                            // original generic args (substituted).  The
                            // replacement's own args are discarded because
                            // the source type provides its own parameters.
                            PhpType::Generic(
                                base.clone(),
                                args.iter().map(|a| a.substitute(subs)).collect(),
                            )
                        }
                        // For non-class replacements (union, intersection,
                        // etc.), the generic wrapper is meaningless — return
                        // the replacement as-is.
                        _ => replacement.clone(),
                    }
                } else {
                    PhpType::Generic(
                        name.clone(),
                        args.iter().map(|a| a.substitute(subs)).collect(),
                    )
                }
            }

            PhpType::Array(inner) => PhpType::Array(Box::new(inner.substitute(subs))),

            PhpType::ArrayShape(entries) => PhpType::ArrayShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.substitute(subs),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::ObjectShape(entries) => PhpType::ObjectShape(
                entries
                    .iter()
                    .map(|e| ShapeEntry {
                        key: e.key.clone(),
                        value_type: e.value_type.substitute(subs),
                        optional: e.optional,
                    })
                    .collect(),
            ),

            PhpType::Callable {
                kind,
                params,
                return_type,
            } => PhpType::Callable {
                kind: kind.clone(),
                params: params
                    .iter()
                    .map(|p| CallableParam {
                        type_hint: p.type_hint.substitute(subs),
                        optional: p.optional,
                        variadic: p.variadic,
                    })
                    .collect(),
                return_type: return_type.as_ref().map(|r| Box::new(r.substitute(subs))),
            },

            PhpType::Conditional {
                param,
                negated,
                condition,
                then_type,
                else_type,
            } => PhpType::Conditional {
                param: param.clone(),
                negated: *negated,
                condition: Box::new(condition.substitute(subs)),
                then_type: Box::new(then_type.substitute(subs)),
                else_type: Box::new(else_type.substitute(subs)),
            },

            PhpType::ClassString(inner) => {
                PhpType::ClassString(inner.as_ref().map(|t| Box::new(t.substitute(subs))))
            }

            PhpType::InterfaceString(inner) => {
                PhpType::InterfaceString(inner.as_ref().map(|t| Box::new(t.substitute(subs))))
            }

            PhpType::KeyOf(inner) => {
                let resolved = inner.substitute(subs);
                evaluate_key_of(&resolved)
            }

            PhpType::ValueOf(inner) => {
                let resolved = inner.substitute(subs);
                evaluate_value_of(&resolved)
            }

            PhpType::IndexAccess(base, index) => {
                let resolved_base = base.substitute(subs);
                let resolved_index = index.substitute(subs);
                evaluate_index_access(&resolved_base, &resolved_index)
            }
        }
    }

    /// Extract all class-like names from this type, recursively.
    ///
    /// Walks the entire type tree and collects the base names of all
    /// class-like types (including those nested inside generics,
    /// callables, shapes, etc.). Scalar types, keywords, `null`,
    /// and literals are skipped.
    ///
    /// For `Collection<int, User>|null`, returns `["Collection", "User"]`.
    /// For `?User`, returns `["User"]`.
    /// For `int|string`, returns `[]`.
    pub fn extract_class_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_class_names(&mut names);
        names
    }

    /// Extract only top-level class names from this type.
    ///
    /// Unlike [`extract_class_names`], this does **not** recurse into
    /// generic type arguments, callable parameters, shape entries, or
    /// other nested positions. It returns only the outermost class
    /// names that are directly part of the type expression.
    ///
    /// For `Collection<int, User>|null`, returns `["Collection"]`.
    /// For `User|Admin`, returns `["User", "Admin"]`.
    /// For `?User`, returns `["User"]`.
    /// For `User[]`, returns `["User"]`.
    /// For `int|string`, returns `[]`.
    ///
    /// This is the correct replacement for the string-based
    /// `extract_class_names_from_type_string` in
    /// `definition/type_definition.rs`, where go-to-type-definition
    /// should jump to the container class, not its type arguments.
    pub fn top_level_class_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        self.collect_top_level_class_names(&mut names);
        names
    }

    /// Recursive helper for [`extract_class_names`].
    fn collect_class_names(&self, names: &mut Vec<String>) {
        match self {
            PhpType::Named(s) => {
                if !is_keyword_type(s) && !s.is_empty() && !names.contains(s) {
                    names.push(s.clone());
                }
            }

            PhpType::Nullable(inner) => inner.collect_class_names(names),

            PhpType::Union(types) | PhpType::Intersection(types) => {
                for t in types {
                    t.collect_class_names(names);
                }
            }

            PhpType::Generic(name, args) => {
                if !is_keyword_type(name) && !name.is_empty() && !names.contains(name) {
                    names.push(name.clone());
                }
                for a in args {
                    a.collect_class_names(names);
                }
            }

            PhpType::Array(inner) => inner.collect_class_names(names),

            PhpType::ArrayShape(entries) | PhpType::ObjectShape(entries) => {
                for e in entries {
                    e.value_type.collect_class_names(names);
                }
            }

            PhpType::Callable {
                params,
                return_type,
                ..
            } => {
                for p in params {
                    p.type_hint.collect_class_names(names);
                }
                if let Some(ret) = return_type {
                    ret.collect_class_names(names);
                }
            }

            PhpType::ClassString(inner) => {
                if let Some(t) = inner {
                    t.collect_class_names(names);
                }
            }

            PhpType::InterfaceString(inner) => {
                if let Some(t) = inner {
                    t.collect_class_names(names);
                }
            }

            PhpType::KeyOf(inner) | PhpType::ValueOf(inner) => {
                inner.collect_class_names(names);
            }

            PhpType::IndexAccess(base, index) => {
                base.collect_class_names(names);
                index.collect_class_names(names);
            }

            PhpType::Conditional {
                condition,
                then_type,
                else_type,
                ..
            } => {
                condition.collect_class_names(names);
                then_type.collect_class_names(names);
                else_type.collect_class_names(names);
            }

            PhpType::Literal(_) | PhpType::Raw(_) | PhpType::IntRange(_, _) => {}
        }
    }

    /// Recursive helper for [`top_level_class_names`].
    ///
    /// Only descends through union, intersection, and nullable
    /// wrappers. Does not recurse into generic args, callable
    /// params/return, shapes, class-string inner types, etc.
    fn collect_top_level_class_names(&self, names: &mut Vec<String>) {
        match self {
            PhpType::Named(s) if !is_keyword_type(s) && !s.is_empty() && !names.contains(s) => {
                names.push(s.clone());
            }

            PhpType::Nullable(inner) => inner.collect_top_level_class_names(names),

            PhpType::Union(types) | PhpType::Intersection(types) => {
                for t in types {
                    t.collect_top_level_class_names(names);
                }
            }

            // For generics, only the base name is top-level.
            // `Collection<int, User>` → `["Collection"]`.
            PhpType::Generic(name, _)
                if !is_keyword_type(name) && !name.is_empty() && !names.contains(name) =>
            {
                names.push(name.clone());
            }

            // `User[]` — the inner type is the top-level class.
            PhpType::Array(inner) => inner.collect_top_level_class_names(names),

            // Shapes, callables, class-string, key-of, value-of,
            // conditionals, literals, int-ranges — no navigable
            // top-level class name.
            _ => {}
        }
    }
}
