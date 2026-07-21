//! `ResolvedType` resolution, narrowing, and join logic.

use super::*;

impl ResolvedType {
    /// Create a `ResolvedType` from a [`ClassInfo`], using its name as
    /// the type string.
    ///
    /// Use this when the original type string is not available (e.g.
    /// when a deep helper returns only `ClassInfo`).  The type string
    /// will be the class name, which is correct for non-generic types
    /// but loses generic parameters.  Future sprints will populate the
    /// type string from the actual return type annotation.
    pub fn from_class(class: ClassInfo) -> Self {
        let type_string = PhpType::Named(class.fqn().to_string());
        Self {
            type_string,
            class_info: Some(Arc::new(class)),
        }
    }

    /// Create a `ResolvedType` from an `Arc<ClassInfo>`, using its name
    /// as the type string.  Avoids cloning when the caller already holds
    /// an `Arc`.
    pub fn from_arc(class: Arc<ClassInfo>) -> Self {
        let type_string = PhpType::Named(class.fqn().to_string());
        Self {
            type_string,
            class_info: Some(class),
        }
    }

    /// Create a `ResolvedType` from a type string with no associated
    /// class info.
    ///
    /// Use this for scalar types (`"int"`, `"string"`), array shapes
    /// (`"array{name: string}"`), and other non-class types.
    pub fn from_type_string(type_string: PhpType) -> Self {
        Self {
            type_string,
            class_info: None,
        }
    }

    /// Create a `ResolvedType` carrying both a type string and a
    /// [`ClassInfo`].
    ///
    /// Use this when the original type string is available (e.g. the
    /// return type annotation of a method).  The type string preserves
    /// generic parameters that would otherwise be lost when resolving
    /// to `ClassInfo`.
    pub fn from_both(type_string: PhpType, class: ClassInfo) -> Self {
        Self {
            type_string,
            class_info: Some(Arc::new(class)),
        }
    }

    /// Create a `ResolvedType` carrying both a type string and an
    /// `Arc<ClassInfo>`.  Avoids cloning when the caller already holds
    /// an `Arc`.
    pub fn from_both_arc(type_string: PhpType, class: Arc<ClassInfo>) -> Self {
        Self {
            type_string,
            class_info: Some(class),
        }
    }

    /// Strip null from the type, preserving class info (since
    /// null-stripping never invalidates the class).
    #[allow(dead_code)]
    pub(crate) fn strip_null(&mut self) {
        if let Some(non_null) = self.type_string.non_null_type() {
            self.type_string = non_null;
        }
    }

    /// Replace the type string and clear `class_info` when the new type
    /// no longer matches the original class.
    pub(crate) fn replace_type(&mut self, new_type: PhpType) {
        let still_matches = self.class_info.as_ref().is_some_and(|ci| {
            // Check base_name first (fast path for simple Named/Generic types).
            if let Some(bn) = new_type.base_name() {
                let bn = bn.strip_prefix('\\').unwrap_or(bn);
                if bn == ci.name || bn == ci.fqn() {
                    return true;
                }
            }
            // For unions/intersections, check whether the class still
            // appears as a top-level member (e.g. `Foobar|int` still
            // contains `Foobar`).
            new_type.top_level_class_names().iter().any(|name| {
                let name = name.strip_prefix('\\').unwrap_or(name);
                name == ci.name || name == ci.fqn()
            })
        });
        if !still_matches {
            self.class_info = None;
        }
        self.type_string = new_type;
    }

    /// Extract just the class info, discarding the type string.
    ///
    /// Convenience method for callers that only need the `ClassInfo`
    /// (e.g. the completion builder).
    pub fn into_class_info(self) -> Option<Arc<ClassInfo>> {
        self.class_info
    }

    /// Push a `ResolvedType` into `results` only if no existing entry
    /// shares the same class name (when both have class info) or the
    /// same type string (when comparing non-class types).
    pub(crate) fn push_unique(results: &mut Vec<ResolvedType>, rt: ResolvedType) {
        let dominated =
            results
                .iter()
                .any(|existing| match (&existing.class_info, &rt.class_info) {
                    (Some(a), Some(b)) => a.name == b.name,
                    (None, None) => existing.type_string == rt.type_string,
                    _ => false,
                });
        if !dominated {
            results.push(rt);
        }
    }

    /// Extend `results` with entries from `new`, skipping duplicates.
    pub(crate) fn extend_unique(results: &mut Vec<ResolvedType>, new: Vec<ResolvedType>) {
        for rt in new {
            Self::push_unique(results, rt);
        }
    }

    /// Convert a `Vec<ClassInfo>` into `Vec<ResolvedType>`, using each
    /// class's name as the type string.
    ///
    /// This is a migration helper for code paths that still produce
    /// `Vec<ClassInfo>` internally (e.g. `type_hint_to_classes_typed`).
    /// Future sprints will populate proper type strings at the source.
    pub(crate) fn from_classes(classes: Vec<Arc<ClassInfo>>) -> Vec<ResolvedType> {
        classes.into_iter().map(ResolvedType::from_arc).collect()
    }

    /// Convert a `Vec<ClassInfo>` into `Vec<ResolvedType>`, preserving
    /// the original type hint string.
    ///
    /// When exactly one class was resolved, the full `type_hint` is
    /// attached (preserving generics like `"Collection<int, User>"`).
    /// When multiple classes were resolved (union split by
    /// `type_hint_to_classes_typed`), each class uses its own name as the
    /// type string because the hint was already split into parts.
    pub(crate) fn from_classes_with_hint(
        classes: Vec<Arc<ClassInfo>>,
        type_hint: PhpType,
    ) -> Vec<ResolvedType> {
        if classes.len() == 1 {
            let class = classes.into_iter().next().unwrap();
            vec![ResolvedType::from_both_arc(type_hint, class)]
        } else if matches!(&type_hint, PhpType::Intersection(_)) {
            // Intersection types: all classes contribute members to a
            // single value.  Emit one ResolvedType per class (so
            // `into_arced_classes` sees every member set) but tag each
            // entry with the full intersection PhpType so that
            // `types_joined` can reconstruct the intersection instead
            // of wrapping them in a union.
            classes
                .into_iter()
                .map(|c| ResolvedType::from_both_arc(type_hint.clone(), c))
                .collect()
        } else {
            let mut results: Vec<ResolvedType> =
                classes.into_iter().map(ResolvedType::from_arc).collect();

            // When the original type hint is a union or nullable,
            // preserve non-class members (scalars like `int`, `string`,
            // `null`) as explicit `ResolvedType` entries so that type
            // guard narrowing (e.g. `is_object()`, `is_int()`,
            // `is_null()`) can filter them like any other union member.
            // Without this, `int` in `Foo|Bar|int` or `null` in
            // `Foo|null` would be silently dropped because they have
            // no ClassInfo.
            let class_fqns: Vec<String> = results
                .iter()
                .filter_map(|rt| rt.class_info.as_ref().map(|c| c.fqn().to_string()))
                .collect();
            let extra_members: Vec<PhpType> = match &type_hint {
                PhpType::Nullable(_) => vec![PhpType::null()],
                PhpType::Union(members) => members
                    .iter()
                    .filter(|m| {
                        // Keep members that were not resolved to a class.
                        match m {
                            PhpType::Named(n) => {
                                let stripped = n.strip_prefix('\\').unwrap_or(n);
                                !class_fqns.iter().any(|fqn| {
                                    fqn == stripped || crate::util::short_name(fqn) == stripped
                                })
                            }
                            _ => true,
                        }
                    })
                    .cloned()
                    .collect(),
                _ => vec![],
            };
            for member in extra_members {
                results.push(ResolvedType::from_type_string(member));
            }

            results
        }
    }

    /// Extract `Vec<ClassInfo>` from `Vec<ResolvedType>`, discarding
    /// entries that have no class info.
    ///
    /// This is a migration helper for callers that currently expect
    /// `Vec<ClassInfo>`.
    #[cfg(test)]
    pub(crate) fn into_classes(resolved: Vec<ResolvedType>) -> Vec<ClassInfo> {
        resolved
            .into_iter()
            .filter_map(|rt| rt.class_info.map(Arc::unwrap_or_clone))
            .collect()
    }

    /// Extract `Vec<Arc<ClassInfo>>` from `Vec<ResolvedType>`, returning
    /// the inner `Arc`s directly (no wrapping needed since `class_info`
    /// is already `Arc<ClassInfo>`).
    ///
    /// This is the primary conversion used by callers of
    /// `resolve_target_classes` that need `Arc<ClassInfo>` for
    /// downstream resolution (completion, hover, definition, etc.).
    pub(crate) fn into_arced_classes(resolved: Vec<ResolvedType>) -> Vec<Arc<ClassInfo>> {
        resolved
            .into_iter()
            .filter_map(|rt| rt.class_info)
            .collect()
    }

    /// Run a narrowing function that operates on `&mut Vec<ClassInfo>`
    /// against a `Vec<ResolvedType>`, preserving type strings.
    ///
    /// Narrowing functions (instanceof, assert, custom type guards)
    /// work on `ClassInfo` values — they add, remove, or replace
    /// classes in the result set based on runtime type checks.  This
    /// adapter extracts the `ClassInfo` layer, runs the narrowing
    /// closure, then reconciles the `ResolvedType` vec:
    ///
    ///   - Entries whose class was removed by narrowing are dropped.
    ///   - Entries that narrowing introduced (e.g. instanceof narrows
    ///     to a new class) are added via `from_class`.
    ///   - Non-class entries (scalars, shapes) are kept unchanged —
    ///     narrowing never affects them, UNLESS `f` reports a definite
    ///     (inclusion-style) narrowing (return `true`), in which case
    ///     leftover non-class `mixed` entries are dropped too — see
    ///     below.
    ///
    /// `f` returns whether it applied a *definite* (inclusion-style)
    /// narrowing — one that concludes the variable's type outright
    /// (e.g. `instanceof` proving membership), as opposed to an
    /// *exclusion*-style narrowing that only rules out one possibility
    /// and leaves the rest of the union (including an unresolved
    /// `mixed` component) intact.
    pub(crate) fn apply_narrowing(
        results: &mut Vec<ResolvedType>,
        f: impl FnOnce(&mut Vec<ClassInfo>) -> bool,
    ) {
        let mut classes: Vec<ClassInfo> = results
            .iter()
            .filter_map(|rt| rt.class_info.as_ref().map(|arc| arc.as_ref().clone()))
            .collect();
        let definite = f(&mut classes);

        // Remove entries whose class was removed by narrowing.
        // Compare by FQN (namespace + name) so that same-named classes
        // from different namespaces (e.g. Contracts\Provider vs
        // Concrete\Provider) are correctly distinguished.
        results.retain(|rt| match &rt.class_info {
            Some(c) => classes.iter().any(|nc| nc.fqn() == c.fqn()),
            // Non-class entries (scalars, shapes) are never affected
            // by narrowing — keep them.
            None => true,
        });

        // Add entries that narrowing introduced (e.g. instanceof
        // narrows to a new class that wasn't in the original set).
        let mut added_new = false;
        for cls in classes {
            if !results
                .iter()
                .any(|rt| rt.class_info.as_ref().is_some_and(|c| c.fqn() == cls.fqn()))
            {
                results.push(ResolvedType::from_class(cls));
                added_new = true;
            }
        }

        // Once narrowing has definitely constrained the value to a
        // specific class, `mixed` is no longer an accurate remaining
        // possibility and would cause false-positive diagnostics after
        // branch merges (where subsumption lets `mixed` swallow the
        // narrowed class type).  `mixed` is kept by the `None => true`
        // retain branch above because it has no `class_info`, so it
        // must be dropped explicitly here.
        //
        // This fires both when narrowing introduced a class that
        // wasn't previously present (`added_new`) and whenever `f`
        // reports a definite (inclusion-style) conclusion (`definite`)
        // — the latter also covers the case where the narrowed class
        // was already one of several possibilities (e.g. a union of a
        // known class and an unresolved `mixed` component), which
        // `added_new` alone cannot detect.
        if added_new || definite {
            results.retain(|rt| !(rt.class_info.is_none() && rt.type_string.is_mixed()));
        }
    }

    /// Combine the type strings of all entries into a single [`PhpType`].
    ///
    /// When there is exactly one entry, returns its `type_string` directly.
    /// When there are multiple entries, wraps them in a [`PhpType::Union`].
    /// When the slice is empty, returns `PhpType::Named("mixed")` as a
    /// safe fallback (callers should check emptiness beforehand).
    ///
    /// Callers that need a display string can use `.to_string()` on the
    /// result, which produces the same `|`-joined output that the former
    /// `type_strings_joined` helper returned, but preserves the structured
    /// [`PhpType`] for any intermediate consumers that benefit from it.
    pub(crate) fn types_joined(resolved: &[ResolvedType]) -> PhpType {
        match resolved.len() {
            0 => PhpType::mixed(),
            1 => resolved[0].type_string.clone(),
            _ => {
                // When all entries share the same intersection type,
                // they came from a single intersection — return it
                // directly instead of wrapping in a Union.
                if let PhpType::Intersection(_) = &resolved[0].type_string
                    && resolved
                        .iter()
                        .all(|rt| rt.type_string == resolved[0].type_string)
                {
                    return resolved[0].type_string.clone();
                }
                let members: Vec<PhpType> =
                    resolved.iter().map(|rt| rt.type_string.clone()).collect();
                PhpType::Union(members)
            }
        }
    }
}
