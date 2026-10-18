//! The parts of a compound type: union and intersection members, an
//! iterable's key and value types, and a callable's signature.

use super::*;

impl PhpType {
    /// Return the top-level union members if this is a union type,
    /// or a single-element slice containing `self` otherwise.
    pub fn union_members(&self) -> Vec<&PhpType> {
        match self.kind() {
            TypeKind::Union(members) => members.iter().collect(),
            _ => vec![self],
        }
    }

    /// Return the top-level intersection members if this is an intersection
    /// type, or a single-element slice containing `self` otherwise.
    pub fn intersection_members(&self) -> Vec<&PhpType> {
        match self.kind() {
            TypeKind::Intersection(members) => members.iter().collect(),
            _ => vec![self],
        }
    }

    /// Extract the "value" type from a generic iterable type.
    ///
    /// Returns the element type that iteration would yield as a value:
    ///   - `User[]`                        → `Some(Named("User"))`
    ///   - `list<User>`                    → `Some(Named("User"))`
    ///   - `array<int, User>`              → `Some(Named("User"))`
    ///   - `Collection<int, User>`         → `Some(Named("User"))`
    ///   - `Generator<int, User, …>`       → `Some(Named("User"))` (2nd param)
    ///   - `?list<User>`                   → `Some(Named("User"))`
    ///   - `int`                           → `None`
    ///
    /// When `skip_scalar` is true, returns `None` if the extracted type
    /// is a scalar (for class-based completion). When false, returns any
    /// element type (matching `extract_iterable_element_type` behaviour).
    pub fn extract_value_type(&self, skip_scalar: bool) -> Option<&PhpType> {
        match self.kind() {
            TypeKind::Array(inner) => {
                if skip_scalar && inner.is_scalar() {
                    None
                } else {
                    Some(inner)
                }
            }
            TypeKind::Generic(g) if !g.args.is_empty() => {
                let args = &g.args;
                // Iterables follow the `<TKey, TValue>` convention: the value
                // is the *second* generic argument whenever two or more are
                // present. This covers `array<K, V>`, `Collection<K, V>`,
                // `Iterator<K, V>`, `Generator<TKey, TValue, TSend, TReturn>`,
                // and the SPL wrapper iterators
                // (`IteratorIterator`/`FilterIterator`/`AppendIterator`) that
                // append a third `TIterator` argument. With a single argument
                // (e.g. `list<User>`) that lone argument is the value.
                let value = if args.len() >= 2 {
                    Some(&args[1])
                } else {
                    args.last()
                };
                match value {
                    Some(v) if skip_scalar && v.is_scalar_leaf() => None,
                    Some(v) => Some(v),
                    None => None,
                }
            }
            TypeKind::Nullable(inner) => inner.extract_value_type(skip_scalar),
            TypeKind::Union(members) => members
                .iter()
                .find_map(|m| m.extract_value_type(skip_scalar)),
            _ => None,
        }
    }

    /// Extract the "key" type from a generic iterable type.
    ///
    /// Returns the key type only when the generic has 2+ parameters:
    ///   - `array<string, User>`  → `Some(Named("string"))`
    ///   - `array<int, User>`     → `Some(Named("int"))`
    ///   - `list<User>`           → `None` (single param → implicit int key)
    ///   - `User[]`               → `None` (shorthand → implicit int key)
    ///
    /// When `skip_scalar` is true, returns `None` if the key type is
    /// scalar.
    pub fn extract_key_type(&self, skip_scalar: bool) -> Option<&PhpType> {
        match self.kind() {
            TypeKind::Generic(g) if g.args.len() >= 2 => {
                let key = &g.args[0];
                if skip_scalar && key.is_scalar() {
                    None
                } else {
                    Some(key)
                }
            }
            TypeKind::Nullable(inner) => inner.extract_key_type(skip_scalar),
            TypeKind::Union(members) => {
                members.iter().find_map(|m| m.extract_key_type(skip_scalar))
            }
            _ => None,
        }
    }

    /// Return the key type produced by iterating this type, as an owned type.
    ///
    /// Unlike [`extract_key_type`](Self::extract_key_type), this joins key
    /// domains from every union member and makes the implicit integer keys of
    /// lists and `T[]` explicit.
    pub fn iterable_key_type(&self) -> Option<PhpType> {
        match self.kind() {
            TypeKind::Array(_) => Some(PhpType::int()),
            TypeKind::ArrayShape(entries) => {
                let keys: Vec<PhpType> = entries
                    .iter()
                    .map(|entry| match entry.key.as_deref() {
                        None => PhpType::int(),
                        // A class-constant key is stored as its spelling.
                        // `Foo::class` is the class's name; any other
                        // constant, not being read here, may be an int or a
                        // string.
                        Some(key) if let Some(class) = class_name_key(key) => {
                            PhpType::class_string(Some(PhpType::named(atom(class))))
                        }
                        Some(key) if key.contains("::") => {
                            PhpType::union(vec![PhpType::int(), PhpType::string()])
                        }
                        Some(key) if is_decimal_int_array_key(key) => PhpType::int(),
                        Some(_) => PhpType::string(),
                    })
                    .collect();
                match keys.len() {
                    0 => None,
                    1 => keys.into_iter().next(),
                    _ => Some(PhpType::join_runtime_value_types(keys)),
                }
            }
            TypeKind::ObjectShape(entries) => {
                // Object property names, including numeric-looking ones, are
                // yielded as strings and never use PHP array-key coercion.
                (!entries.is_empty()).then(PhpType::string)
            }
            TypeKind::Generic(g) if g.args.len() >= 2 => Some(g.args[0].clone()),
            // A list's keys count up from zero.
            TypeKind::Generic(g)
                if g.args.len() == 1
                    && matches!(
                        g.name.to_ascii_lowercase().as_str(),
                        "list" | "non-empty-list"
                    ) =>
            {
                Some(PhpType::int_range("0", "max"))
            }
            // A single-argument `array<T>` names its value type, leaving
            // the same implicit integer keys as `T[]`.
            TypeKind::Generic(g)
                if g.args.len() == 1
                    && matches!(
                        g.name.to_ascii_lowercase().as_str(),
                        "array" | "non-empty-array"
                    ) =>
            {
                Some(PhpType::int())
            }
            TypeKind::Nullable(inner) => inner.iterable_key_type(),
            TypeKind::Union(members) => {
                let keys: Vec<PhpType> = members
                    .iter()
                    .filter_map(PhpType::iterable_key_type)
                    .collect();
                match keys.len() {
                    0 => None,
                    1 => keys.into_iter().next(),
                    _ => Some(PhpType::join_runtime_value_types(keys)),
                }
            }
            _ => None,
        }
    }

    /// Whether this type only names a value type, leaving the key domain
    /// open: `array<T>`, `non-empty-array<T>`, `T[]` and bare `array`.
    ///
    /// [`iterable_key_type`](Self::iterable_key_type) answers `int` for these
    /// because sequential keys are the common case and iterating them as
    /// `int` is the useful default. That default is a guess, though, and a
    /// caller that is about to *refine* the key domain (rather than read one
    /// element out of it) has to start from every key PHP allows, or the
    /// refinement contradicts the guess and is thrown away. `list<T>` is not
    /// open: it promises `int` keys.
    pub fn has_open_key_domain(&self) -> bool {
        match self.kind() {
            TypeKind::Array(_) => true,
            TypeKind::Generic(g) => {
                g.args.len() == 1
                    && matches!(
                        g.name.to_ascii_lowercase().as_str(),
                        "array" | "non-empty-array"
                    )
            }
            TypeKind::Named(name) => {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "array" | "non-empty-array"
                )
            }
            TypeKind::Nullable(inner) => inner.has_open_key_domain(),
            TypeKind::Union(members) => members.iter().any(PhpType::has_open_key_domain),
            _ => false,
        }
    }

    /// The generic array type a constant shape describes, dropping the
    /// per-key detail: `array{a: int, b: string}` → `array<string,
    /// int|string>`, `array{User, Order}` → `list<User|Order>`.
    ///
    /// Anything that is not an array shape is returned unchanged, so a
    /// caller that only needs a container it can read a key and value type
    /// off can pass any type through. An empty shape has no key or value
    /// type to name and becomes a bare `array`.
    pub fn generalized_array(&self) -> PhpType {
        let TypeKind::ArrayShape(entries) = self.kind() else {
            return self.clone();
        };
        if entries.is_empty() {
            return PhpType::array();
        }
        let Some(value) = self.iterable_element_type() else {
            return PhpType::array();
        };
        // Only an all-positional shape promises the `0, 1, 2, …` keys a
        // `list` does; the moment one entry is named the result has to
        // spell its key type out.
        if entries.iter().all(|e| e.key.is_none()) {
            return PhpType::list(value);
        }
        match self.iterable_key_type() {
            Some(key) => PhpType::generic_array(key, value),
            None => PhpType::generic_array_val(value),
        }
    }

    /// Extract the element (value) type from an iterable, including
    /// scalar element types.
    ///
    /// This is the `PhpType` equivalent of `extract_iterable_element_type`.
    /// Unlike `extract_value_type(true)`, this never skips scalars.
    pub fn extract_element_type(&self) -> Option<&PhpType> {
        self.extract_value_type(false)
    }

    /// Return the element (value) type produced by iterating this type,
    /// as an owned type.
    ///
    /// Unlike [`extract_value_type`](Self::extract_value_type), this also
    /// handles array/object shapes: iterating a tuple-style `array{A, B}`
    /// yields `A|B`. For all other types it delegates to
    /// `extract_value_type(false)`, so generic collections (`list<User>`,
    /// `array<int, Order>`) behave exactly as before.
    pub fn iterable_element_type(&self) -> Option<PhpType> {
        match self.kind() {
            TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => {
                let mut values: Vec<PhpType> = Vec::new();
                for entry in entries {
                    if !values.contains(&entry.value_type) {
                        values.push(entry.value_type.clone());
                    }
                }
                match values.len() {
                    0 => None,
                    1 => Some(values.into_iter().next().unwrap()),
                    _ => Some(PhpType::union(values)),
                }
            }
            TypeKind::Nullable(inner) => inner.iterable_element_type(),
            TypeKind::Union(members) => {
                let values: Vec<PhpType> = members
                    .iter()
                    .filter_map(PhpType::iterable_element_type)
                    .collect();
                match values.len() {
                    0 => None,
                    1 => values.into_iter().next(),
                    _ => Some(PhpType::join_runtime_value_types(values)),
                }
            }
            _ => self.extract_value_type(false).cloned(),
        }
    }

    /// Extract parameter types from a `Callable` variant.
    ///
    /// Returns the parameter list for callable/Closure types without
    /// round-tripping through string serialization.
    ///
    ///   - `callable(int, string): bool` → `Some(&[CallableParam { .. }, ..])`
    ///   - `?Closure(int): void`         → `Some(&[CallableParam { .. }])`
    ///   - `Closure(int)|null`           → `Some(&[CallableParam { .. }])`
    ///   - `int`                         → `None`
    pub fn callable_param_types(&self) -> Option<&[CallableParam]> {
        match self.kind() {
            TypeKind::Callable(c) => Some(c.params.as_slice()),
            TypeKind::Nullable(inner) => inner.callable_param_types(),
            TypeKind::Union(members) => {
                for member in members {
                    if let Some(params) = member.callable_param_types() {
                        return Some(params);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Extract the return type from a `Callable` variant.
    ///
    /// Returns the return type for callable/Closure types without
    /// round-tripping through string serialization.
    ///
    ///   - `callable(int): User`  → `Some(Named("User"))`
    ///   - `Closure(): void`      → `Some(Named("void"))`
    ///   - `?Closure(): User`     → `Some(Named("User"))`
    ///   - `callable`             → `None` (no return type specified)
    ///   - `int`                  → `None`
    pub fn callable_return_type(&self) -> Option<&PhpType> {
        match self.kind() {
            TypeKind::Callable(c) => c.return_type.as_ref(),
            TypeKind::Nullable(inner) => inner.callable_return_type(),
            TypeKind::Union(members) => {
                for member in members {
                    if let Some(ret) = member.callable_return_type() {
                        return Some(ret);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Extract the TSend type (3rd generic parameter) from a Generator.
    ///
    /// `Generator<TKey, TValue, TSend, TReturn>` — the send type is the
    /// 3rd parameter (index 2).
    ///
    ///   - `Generator<int, string, MyClass, void>` → `Some(Named("MyClass"))`
    ///   - `?Generator<int, string, MyClass, void>` → `Some(Named("MyClass"))`
    ///   - `Generator<int, string>`                 → `None` (fewer than 3 params)
    ///   - `int`                                    → `None`
    ///
    /// When `skip_scalar` is true, returns `None` if the send type is
    /// scalar (matching the pattern used by `extract_value_type`).
    pub fn generator_send_type(&self, skip_scalar: bool) -> Option<&PhpType> {
        match self.kind() {
            TypeKind::Generic(g) if Self::short_name_of(&g.name) == "Generator" => {
                match g.args.get(2) {
                    Some(send) if skip_scalar && send.is_scalar() => None,
                    Some(send) => Some(send),
                    None => None,
                }
            }
            TypeKind::Nullable(inner) => inner.generator_send_type(skip_scalar),
            _ => None,
        }
    }
}
