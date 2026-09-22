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
        let type_string = PhpType::named(class.fqn());
        Self {
            type_string,
            class_info: Some(Arc::new(class)),
            factory_count: FactoryCount::Unknown,
        }
    }

    /// Create a `ResolvedType` from an `Arc<ClassInfo>`, using its name
    /// as the type string.  Avoids cloning when the caller already holds
    /// an `Arc`.
    pub fn from_arc(class: Arc<ClassInfo>) -> Self {
        let type_string = PhpType::named(class.fqn());
        Self {
            type_string,
            class_info: Some(class),
            factory_count: FactoryCount::Unknown,
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
            factory_count: FactoryCount::Unknown,
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
            factory_count: FactoryCount::Unknown,
        }
    }

    /// Create a `ResolvedType` carrying both a type string and an
    /// `Arc<ClassInfo>`.  Avoids cloning when the caller already holds
    /// an `Arc`.
    pub fn from_both_arc(type_string: PhpType, class: Arc<ClassInfo>) -> Self {
        Self {
            type_string,
            class_info: Some(class),
            factory_count: FactoryCount::Unknown,
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

    /// Drop the `type_string` union alternatives that a definite
    /// narrowing to the classes `survives` accepts has ruled out.
    ///
    /// One entry's `type_string` can carry a whole union while its
    /// `class_info` names a single class — a conditional return type
    /// such as `UploadedFile|array<UploadedFile>|null` resolves that
    /// way.  Narrowing that works on the `class_info` layer therefore
    /// leaves the ruled-out members in place unless it comes back
    /// through here.
    pub(crate) fn restrict_type_string_to_classes(&mut self, survives: &impl Fn(&str) -> bool) {
        if let Some(restricted) = restrict_union_to_classes(&self.type_string, survives) {
            self.type_string = restricted;
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
    /// shares the same class FQN (when both have class info) or the
    /// same type string (when comparing non-class types).
    ///
    /// Keying on the FQN rather than the short name keeps `NsA\Thing` and
    /// `NsB\Thing` apart, so a union over both retains the members of each.
    pub(crate) fn push_unique(results: &mut Vec<ResolvedType>, rt: ResolvedType) {
        let dominating =
            results
                .iter_mut()
                .find(|existing| match (&existing.class_info, &rt.class_info) {
                    (Some(a), Some(b)) => a.fqn() == b.fqn(),
                    (None, None) => existing.type_string == rt.type_string,
                    _ => false,
                });
        match dominating {
            // The entry that stays speaks for the one that was dropped,
            // so it may only keep a factory count both agreed on.
            Some(existing) => {
                existing.factory_count = existing.factory_count.join(rt.factory_count)
            }
            None => results.push(rt),
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

    /// Give every entry the same intersection `type_string` naming all of
    /// the classes in `results`.
    ///
    /// Narrowing an `instanceof` against a class the subject does not
    /// nominally implement (a mock or dynamic proxy that is the declared
    /// class *and* implements the checked interface) keeps both classes,
    /// because the value satisfies them simultaneously.  Each entry keeps
    /// its own `class_info` so member lookup still finds members from
    /// every class, while the shared intersection `type_string` tells
    /// [`Self::types_joined`] to report `A&B` instead of wrapping the
    /// entries in an `A|B` union — a compatibility check against a
    /// parameter naming just one member has to pass, not be judged
    /// against the other member too.
    ///
    /// A single entry is already unambiguous, and a set with a non-class
    /// entry (a scalar, an array shape) is not an intersection of
    /// classes, so both are left alone.
    pub(crate) fn tag_as_intersection(results: &mut [ResolvedType]) {
        if results.len() < 2 {
            return;
        }
        let Some(members) = results
            .iter()
            .map(|rt| rt.class_info.as_ref().map(|c| PhpType::named(c.fqn())))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let intersection = PhpType::intersection(members);
        for rt in results.iter_mut() {
            rt.type_string = intersection.clone();
        }
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
        } else if matches!(&type_hint.kind(), TypeKind::Intersection(_)) {
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

            // A generic union member (`Collection<int, Order>`) resolves to
            // the class its base name holds, so it belongs on that class's
            // entry rather than beside it as a member of its own — otherwise
            // the value reads as `Collection|Order|Collection<int, Order>`,
            // naming the same collection twice.
            let mut attached: Vec<PhpType> = Vec::new();
            if let TypeKind::Union(members) = type_hint.kind() {
                for member in members {
                    if !matches!(member.kind(), TypeKind::Generic(_)) {
                        continue;
                    }
                    let Some(base) = member.base_name() else {
                        continue;
                    };
                    let entry = results.iter_mut().find(|rt| {
                        !matches!(rt.type_string.kind(), TypeKind::Generic(_))
                            && rt.class_info.as_ref().is_some_and(|c| {
                                let fqn = c.fqn().to_string();
                                fqn == base || crate::util::short_name(&fqn) == base
                            })
                    });
                    if let Some(entry) = entry {
                        entry.type_string = member.clone();
                        attached.push(member.clone());
                    }
                }
            }

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
            let extra_members: Vec<PhpType> = match &type_hint.kind() {
                TypeKind::Nullable(_) => vec![PhpType::null()],
                TypeKind::Union(members) => members
                    .iter()
                    .filter(|m| {
                        // Keep members that were not resolved to a class.
                        match m.kind() {
                            TypeKind::Named(n)
                            | TypeKind::StaticType(n)
                            | TypeKind::ThisType(n) => {
                                let stripped = n.strip_prefix('\\').unwrap_or(n);
                                !class_fqns.iter().any(|fqn| {
                                    fqn == stripped || crate::util::short_name(fqn) == stripped
                                })
                            }
                            _ => !attached.contains(m),
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
        results.retain_mut(|rt| match &rt.class_info {
            Some(c) => {
                if classes.iter().any(|nc| nc.fqn() == c.fqn()) {
                    return true;
                }
                // A definite narrowing concludes the value is one of the
                // surviving classes outright, so an entry it ruled out
                // carries nothing worth keeping.
                if definite {
                    return false;
                }
                // An exclusion only rules out the one class, and the
                // entry's `type_string` can carry union alternatives the
                // `class_info` layer `f` operates on never saw: a union
                // that resolves to exactly one class collapses onto a
                // single entry, so `Decimal|float` is one `ResolvedType`
                // whose `class_info` names `Decimal` alone.  Dropping it
                // would take `float` down with the class the check ruled
                // out, leaving the guarded branch with no type at all
                // instead of the half the check proved.
                let (name, fqn) = (c.name, c.fqn());
                let ruled_out = |member: &str| member == name || member == fqn;
                match subtract_classes_from_union(&rt.type_string, &ruled_out) {
                    Some(rest) => {
                        rt.type_string = rest;
                        rt.class_info = None;
                        true
                    }
                    None => false,
                }
            }
            // Non-class entries (scalars, shapes) are never affected
            // by narrowing — keep them.
            None => true,
        });

        // A definite narrowing proves the value is an instance of one
        // of the surviving classes, so every alternative that is not
        // one of them is ruled out.  A surviving entry can still carry
        // such an alternative out of reach of the `class_info` layer
        // `f` operates on: one conditional return type resolves to a
        // single entry whose `type_string` is the whole union
        // (`UploadedFile|array<UploadedFile>|null`) while its
        // `class_info` names only `UploadedFile`.  Left in place, the
        // array member keeps reaching consumers that read
        // `type_string`, so an argument check after an `instanceof`
        // guard still judges the parameter against it.
        if definite && !classes.is_empty() {
            let survives = |name: &str| classes.iter().any(|c| c.name == name || c.fqn() == name);
            for rt in results.iter_mut() {
                rt.restrict_type_string_to_classes(&survives);
            }
        }

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

    /// Collapse scalar literals made redundant by broader runtime-value
    /// alternatives while preserving class-backed entries and their metadata.
    ///
    /// Branch resolution can return class and scalar alternatives side by
    /// side. Joining the entire vector would discard `class_info`, but leaving
    /// every entry untouched produces unions such as `Foo|'fixed'|string`.
    /// Normalize only the non-class alternatives.
    ///
    /// `mixed` absorbs every literal beside it, and that is the point: a
    /// branch the resolver could not type is `mixed`, and the union it joins
    /// must not go on advertising a sibling literal as if it were the whole
    /// answer. The exception is a `mixed` buried in a compound alternative
    /// such as `mixed|Foo`, where absorbing would drop the `Foo` that carries
    /// the completion fallback.
    pub(crate) fn collapse_redundant_runtime_literals(
        results: Vec<ResolvedType>,
    ) -> Vec<ResolvedType> {
        // `true` and `false` are spelled as keyword names rather than
        // `TypeKind::Literal`, but they are literal values all the same:
        // a branch that assigned `false` beside one that produced `bool`
        // has nothing extra to say, and `true` beside `false` is `bool`.
        fn contains_scalar_literal(ty: &PhpType) -> bool {
            match ty.kind() {
                TypeKind::Literal(_) => true,
                TypeKind::Named(name) => {
                    name.eq_ignore_ascii_case("true") || name.eq_ignore_ascii_case("false")
                }
                TypeKind::Union(members) => members.iter().any(contains_scalar_literal),
                TypeKind::Nullable(inner) => contains_scalar_literal(inner),
                _ => false,
            }
        }

        fn mixed_hides_alternatives(ty: &PhpType) -> bool {
            match ty.kind() {
                TypeKind::Union(members) => {
                    members.len() > 1 && members.iter().any(PhpType::contains_mixed)
                }
                TypeKind::Nullable(inner) => inner.contains_mixed(),
                _ => false,
            }
        }

        let non_class_types: Vec<PhpType> = results
            .iter()
            .filter(|result| result.class_info.is_none())
            .map(|result| result.type_string.clone())
            .collect();
        if non_class_types.is_empty()
            || !non_class_types.iter().any(contains_scalar_literal)
            || non_class_types.iter().any(mixed_hides_alternatives)
        {
            return results;
        }

        // Splitting a union apart to normalize it loses the benevolence
        // marker sitting above it, and a `false` branch is exactly what
        // brings a lenient union through here in the first place. A scope
        // that merely rejoined must not make it enforceable again, so the
        // marker is carried across — but only while every contributing
        // type had it, since one the code did spell out makes the whole
        // union worth enforcing.
        let all_lenient = non_class_types.iter().all(PhpType::is_benevolent);
        let original = match non_class_types.len() {
            1 => non_class_types[0].clone(),
            _ => PhpType::union(non_class_types.clone()),
        };
        let mut normalized = PhpType::join_runtime_value_types(non_class_types);
        if all_lenient {
            normalized = PhpType::benevolent(normalized);
        }
        // Without the marker carried across, this comparison would see two
        // types differing by nothing else and rebuild the entry anyway.
        if normalized == original {
            return results;
        }

        let mut collapsed = Vec::with_capacity(results.len());
        let mut inserted_non_class = false;
        for result in results {
            if result.class_info.is_some() {
                collapsed.push(result);
            } else if !inserted_non_class {
                collapsed.push(ResolvedType::from_type_string(normalized.clone()));
                inserted_non_class = true;
            }
        }
        collapsed
    }

    /// Remove entries whose type is a subset of another entry's, e.g.
    /// `string|null` ⊆ `int|string|null` drops the former.
    ///
    /// Shared by every merge point that produces a `Vec<ResolvedType>` for
    /// one variable/expression from more than one branch: real `if`/`else`
    /// scope joins ([`ScopeState::merge_branch`](crate::type_engine::variable::forward_walk::scope_state::ScopeState::merge_branch))
    /// and single-expression branch merges (ternaries, match arms) alike.
    /// A narrowed type from a branch that does not exit leaks into the
    /// post-merge scope and pollutes subsequent narrowing unless the
    /// broader sibling that already covers it wins here.
    ///
    /// `mixed_absorbs_siblings` controls whether a bare `mixed` entry may
    /// cover (and so drop) any narrower sibling. A real scope join needs
    /// this: `if ($mixed instanceof Foo) { … }` with no `else` must not
    /// leave `Foo` sitting beside `mixed` once the branch's proof no
    /// longer applies (`ScopeState::merge_branch` passes `true`). A
    /// ternary/match-arm value merge must not (pass `false`): the
    /// narrower arm's value is one this expression can actually produce,
    /// not a proof that stopped holding.
    pub(crate) fn drop_subsumed_entries(
        entries: &mut Vec<ResolvedType>,
        mixed_absorbs_siblings: bool,
    ) {
        if entries.len() <= 1 {
            return;
        }
        let types: Vec<PhpType> = entries.iter().map(|rt| rt.type_string.clone()).collect();
        let mut keep = vec![true; types.len()];
        for i in 0..types.len() {
            if !keep[i] {
                continue;
            }
            for j in 0..types.len() {
                if i == j || !keep[j] {
                    continue;
                }
                if types[i].is_mixed() && !types[j].is_mixed() && !mixed_absorbs_siblings {
                    continue;
                }
                // If j is a strict subset of i, drop j.
                if types[j] != types[i]
                    && (types[j].is_subset_of(&types[i])
                        || array_snapshot_covered_by(&types[j], &types[i])
                        || intersection_covered_by(&types[j], &types[i]))
                {
                    keep[j] = false;
                }
            }
        }
        crate::util::retain_by_mask(entries, &keep);
    }

    /// Combine the type strings of all entries into a single [`PhpType`].
    ///
    /// When there is exactly one entry, returns its `type_string` directly.
    /// When there are multiple entries, wraps them in a [`TypeKind::Union`].
    /// When the slice is empty, returns `PhpType::mixed()` as a safe
    /// fallback (callers should check emptiness beforehand).
    ///
    /// Callers that need a display string can call `.to_string()` on the
    /// result, which produces a `|`-joined string while preserving the
    /// structured [`PhpType`] for any intermediate consumers that benefit
    /// from it.
    pub(crate) fn types_joined(resolved: &[ResolvedType]) -> PhpType {
        match resolved.len() {
            0 => PhpType::mixed(),
            1 => resolved[0].type_string.clone(),
            _ => {
                // When all entries share the same intersection type,
                // they came from a single intersection — return it
                // directly instead of wrapping in a Union.
                if let TypeKind::Intersection(_) = &resolved[0].type_string.kind()
                    && resolved
                        .iter()
                        .all(|rt| rt.type_string == resolved[0].type_string)
                {
                    return resolved[0].type_string.clone();
                }
                // An entry that is itself a union contributes its own
                // alternatives rather than nesting: `$name ?? pathinfo(…)`
                // joins a `string` entry with a `string|array{…}` one, and
                // without flattening the repeated `string` is invisible to
                // the union's own deduplication. A marked union is left
                // whole, since the marker (benevolence, list ordering) sits
                // above the union and would be lost by lifting its members
                // out: an unflattened `__benevolent<string|false>` still
                // says its failure branch is not worth enforcing.
                let mut members: Vec<PhpType> = Vec::with_capacity(resolved.len());
                for rt in resolved {
                    match rt.type_string.raw_kind() {
                        TypeKind::Union(alternatives) => {
                            members.extend(alternatives.iter().cloned())
                        }
                        _ => members.push(rt.type_string.clone()),
                    }
                }
                // The `[]` a variable was initialised to says nothing
                // beside the array a later write produced: `array{}` is the
                // empty array, and an array alternative that demands no
                // entry already contains it. Dropping it is what keeps a
                // loop accumulator and a by-ref closure capture from
                // reporting `array{}|list<string>`, whose offset reads then
                // carry a spurious `null` from the empty half.
                if members.iter().any(PhpType::is_empty_array_shape)
                    && members.iter().any(PhpType::accepts_empty_array)
                {
                    members.retain(|m| !m.is_empty_array_shape());
                }
                PhpType::union(members)
            }
        }
    }
}

/// Restrict a union `type_string` to the members naming a class that
/// `survives` accepts, dropping the alternatives a definite class
/// narrowing has ruled out.
///
/// Returns `None` when there is nothing to restrict: a type that is not
/// a union, one whose members all survive, and one where no member
/// names a surviving class (an `instanceof` against a class the union
/// never mentioned narrows to an intersection, which the callers model
/// separately).
fn restrict_union_to_classes(ty: &PhpType, survives: &impl Fn(&str) -> bool) -> Option<PhpType> {
    // `?Foo` carries the null alternative in the wrapper rather than as
    // a union member, so unwrapping it is itself a restriction.
    if let TypeKind::Nullable(inner) = ty.kind() {
        return Some(restrict_union_to_classes(inner, survives).unwrap_or_else(|| inner.clone()));
    }
    // An intersection whose members are themselves unions restricts one
    // member at a time: `(FunctionNode|MethodNode)&MockObject` proven to
    // be a `MethodNode` is `MethodNode&MockObject`. The `MockObject` side
    // names no surviving class and is kept — it is a conjunct the value
    // still satisfies, not an alternative the proof ruled out.
    if let TypeKind::Intersection(members) = ty.kind() {
        let mut restricted = false;
        let narrowed: Vec<PhpType> = members
            .iter()
            .map(|m| match restrict_union_to_classes(m, survives) {
                Some(inner) => {
                    restricted = true;
                    inner
                }
                None => m.clone(),
            })
            .collect();
        return restricted.then(|| PhpType::intersection(narrowed));
    }
    filter_union_members(ty, |m| union_member_names_class(m, survives))
}

/// Drop the `type_string` union alternatives that name a class an
/// *exclusion* narrowing ruled out, returning what is left, or `None`
/// when nothing is.
///
/// The dual of [`restrict_union_to_classes`], and needed for the same
/// reason: a union resolving to one class collapses onto a single entry
/// carrying the whole union in its `type_string`, so narrowing on the
/// `class_info` layer cannot see the alternatives it must preserve.
fn subtract_classes_from_union(ty: &PhpType, ruled_out: &impl Fn(&str) -> bool) -> Option<PhpType> {
    // `?Foo` carries the null alternative in the wrapper rather than as
    // a union member, so ruling `Foo` out leaves a bare `null`.
    if let TypeKind::Nullable(inner) = ty.kind() {
        return match subtract_classes_from_union(inner, ruled_out) {
            Some(rest) => Some(PhpType::nullable(rest)),
            None if union_member_names_class(inner, ruled_out) => Some(PhpType::null()),
            None => None,
        };
    }
    // Nothing left, or the ruled-out class was never named here and the
    // union says nothing about what narrowing concluded.  Either way
    // `filter_union_members` reports `None` and the caller drops the
    // entry, as it did before this refinement.
    filter_union_members(ty, |m| !union_member_names_class(m, ruled_out))
}

/// Keep the union members `keep` accepts, or `None` when that changes
/// nothing: either every member survives, or none does.
///
/// `PhpType::union` does not normalise, so a lone survivor is unwrapped
/// rather than left as a one-member union.
fn filter_union_members(ty: &PhpType, keep: impl Fn(&PhpType) -> bool) -> Option<PhpType> {
    let TypeKind::Union(members) = ty.kind() else {
        return None;
    };
    let kept: Vec<PhpType> = members.iter().filter(|m| keep(m)).cloned().collect();
    if kept.is_empty() || kept.len() == members.len() {
        return None;
    }
    match kept.len() {
        1 => kept.into_iter().next(),
        _ => Some(PhpType::union(kept)),
    }
}

/// Report whether a union member describes one of the surviving
/// classes.  An intersection member counts when any of its parts does:
/// `Foo&Countable` is still a `Foo`.
fn union_member_names_class(member: &PhpType, survives: &impl Fn(&str) -> bool) -> bool {
    if let TypeKind::Intersection(parts) = member.kind() {
        return parts.iter().any(|p| union_member_names_class(p, survives));
    }
    member.base_name().is_some_and(survives)
}

/// Whether an array-like `covered` is already covered by a parameterised
/// array-like `cover`.
///
/// Two branches producing different array snapshots of the same variable
/// (a loop appending an element, an early-return leaving an empty array)
/// and keeping them all produces self-overlapping unions like
/// `array{}|array<int, A>|array<int, A|B>`.  Dropping every snapshot a
/// sibling already covers leaves the single widest one.
///
/// A bare `array` never covers a parameterised sibling: it carries no
/// element information, so collapsing onto it would trade the only useful
/// snapshot for the least useful one.  Two arrays that describe genuinely
/// different values (`array<int, int>` and `array<int, string>` from
/// separate assignments) cover neither way and both survive.
fn array_snapshot_covered_by(covered: &PhpType, cover: &PhpType) -> bool {
    covered.is_array_like()
        && cover.is_array_like()
        && !matches!(cover.kind(), TypeKind::Named(_))
        && covered.is_subtype_of(cover)
}

/// Whether an intersection produced by one branch is already covered by
/// a sibling alternative.
///
/// `if ($r instanceof Verbose) { … }` narrows `$r` to `Base&Verbose` for
/// the length of the branch.  The path that skipped the branch still has
/// a plain `Base`, and every `Base&Verbose` is a `Base`, so the join is
/// `Base` — keeping both would report `Base|Base&Verbose` and leave the
/// branch-local proof visible after the branch it belongs to.
///
/// The covering type may name the part as one alternative among several
/// rather than on its own: the path that skipped the branch above often
/// carries `?Base`, and `Base&Verbose` is still one of the values that
/// describes.
///
/// Only a part named verbatim by the covering type counts.  Widening a
/// part to its parent would need the class loader, and the case that
/// matters (a branch narrowing the very type the sibling path carries)
/// names it exactly.
fn intersection_covered_by(covered: &PhpType, cover: &PhpType) -> bool {
    let TypeKind::Intersection(parts) = covered.kind() else {
        return false;
    };
    parts.iter().any(|part| part.is_subset_of(cover))
}
