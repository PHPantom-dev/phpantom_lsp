//! What a type says about a value's truthiness, and the types a value is
//! narrowed to once its truthiness, emptiness, or nullness is known.

use super::*;

/// The truthiness of a named type, bare or with type arguments.
///
/// A class instance is always truthy, and so is a class name: no class is
/// called `''` or `'0'`.  A `non-empty-string` is not: `'0'` is non-empty
/// and falsy.
fn named_truthiness(name: &str) -> Option<bool> {
    match name.to_ascii_lowercase().as_str() {
        "object" | "non-falsy-string" | "truthy-string" | "non-empty-array" | "non-empty-list"
        | "positive-int" | "negative-int" | "non-zero-int" | "callable" | "closure"
        | "class-string" | "interface-string" | "trait-string" | "enum-string" => Some(true),
        other if is_keyword_type(other) => None,
        _ => Some(true),
    }
}

impl PhpType {
    /// Return the non-null part of a type.
    ///
    /// For a union like `User|null`, returns `Some(Named("User"))`.
    /// For `User|Admin|null`, returns `Some(Union([Named("User"), Named("Admin")]))`.
    /// For a type that doesn't contain `null`, returns `None`.
    /// For bare `null`, returns `None`.
    ///
    /// This extracts the non-null part from a union type.
    pub fn non_null_type(&self) -> Option<PhpType> {
        match self.kind() {
            TypeKind::Nullable(inner) => Some(inner.clone()),
            TypeKind::Union(members) => {
                let non_null: Vec<&PhpType> = members.iter().filter(|m| !m.is_null()).collect();
                match non_null.len() {
                    0 => None,
                    1 => Some(non_null[0].clone()),
                    _ => Some(PhpType::union(non_null.into_iter().cloned().collect())),
                }
            }
            // Not a union or nullable — no null to strip.
            _ => None,
        }
    }

    /// Whether every value of this type is truthy, or none of them is.
    ///
    /// `None` for the types that span both (`string` holds `''`, `int`
    /// holds `0`, `array` holds `[]`), which leaves a caller reasoning
    /// about a truthiness test undecided rather than guessing.
    ///
    /// This is a single atomic member's truthiness: a union or nullable
    /// spans its members and is therefore undecided.
    pub fn truthiness(&self) -> Option<bool> {
        if self.is_null() || self.is_false() {
            return Some(false);
        }
        if self.is_true() {
            return Some(true);
        }
        match self.kind() {
            // PHP's own cast-to-bool rules, read off the literal's value
            // rather than its source text: `''` and `'0'` are the only falsy
            // strings, so `'0.0'` and `' '` are truthy.  A number the parsers
            // cannot read is one no `0` spelling produces, so it counts as
            // truthy rather than leaving the whole branch undecided.
            TypeKind::Literal(value) => Some(match value.as_ref() {
                LiteralValue::String(_) => value
                    .string_content()
                    .is_none_or(|text| !text.is_empty() && text != "0"),
                LiteralValue::Int(_) => value.parse_i64().is_none_or(|value| value != 0),
                LiteralValue::Float(_) => value.parse_f64().is_none_or(|value| value != 0.0),
            }),
            TypeKind::Named(name) => named_truthiness(name),
            // `class-string<Foo>` names a class and `Collection<int>` is an
            // instance of one, so each is truthy whatever its arguments.
            TypeKind::Generic(generic) => named_truthiness(&generic.name),
            TypeKind::ClassString(_) | TypeKind::InterfaceString(_) => Some(true),
            // An object shape or an intersection is an object, and an object
            // is always truthy.  A shape with at least one required field is
            // a non-empty array; an empty one (`array{}`) is falsy.
            TypeKind::Intersection(_) | TypeKind::ObjectShape(_) => Some(true),
            TypeKind::ArrayShape(_) | TypeKind::ListShape(_) => self
                .shape_entries()
                .map(|entries| entries.iter().any(|entry| !entry.optional)),
            _ => None,
        }
    }

    /// Return the part of a type that can be truthy.
    ///
    /// For `string|false` returns `Some(Named("string"))`, for `?User`
    /// `Some(Named("User"))`, and for `null`, `false`, or `?false` `None`,
    /// since nothing those describe survives a truthiness check.
    ///
    /// Members that are *always* falsy are dropped and `bool` keeps only
    /// its `true` half, which is the subset a truthy `if ($x)` branch
    /// narrows a variable to. Refinements a truthy test could also justify
    /// but that PHP has no plain spelling for are left alone: `int` stays
    /// `int` rather than becoming a range excluding `0`.
    pub fn truthy_type(&self) -> Option<PhpType> {
        match self.kind() {
            TypeKind::Nullable(inner) => inner.truthy_type(),
            // Each member is stripped by recursing rather than by checking
            // only whether the member as a whole is certainly falsy: a
            // `?int` member sitting inside a wider union (`?int|?bool`,
            // built by merging several properties into one array's value
            // type) is not certainly falsy on its own, so a shallow check
            // would leave its `null` in place while a bare `?int` handed to
            // this function directly loses it via the `Nullable` arm above.
            // Recursing makes every member go through the exact same rule
            // regardless of what else shares the union with it.
            TypeKind::Union(members) => {
                let truthy: Vec<PhpType> =
                    members.iter().filter_map(PhpType::truthy_type).collect();
                match truthy.len() {
                    0 => None,
                    1 => truthy.into_iter().next(),
                    _ => Some(PhpType::union(truthy)),
                }
            }
            _ if self.truthiness() == Some(false) => None,
            // A `bool` that survives a truthy test can only have been
            // `true`, and keeping it as `bool` makes the check
            // unfalsifiable: a variable seeded `false` and reassigned in
            // one branch would go on carrying its falsy half for the rest
            // of the scope.
            _ if self.is_bool() => Some(PhpType::true_()),
            // `[]` is the only falsy array, so an array that survives a
            // truthy test has at least one entry.  Carrying that in the
            // type is what lets a later `foreach` know its body runs.
            _ if self.is_array_like() => Some(self.non_empty_array_form()),
            // `''` and `'0'` are the falsy strings, so a string that
            // survives is neither.
            TypeKind::Named(name) => Some(match name.as_str() {
                "string" | "non-empty-string" => PhpType::non_falsy_string(),
                "lowercase-string" | "non-empty-lowercase-string" => PhpType::intersection(vec![
                    PhpType::lowercase_string(),
                    PhpType::non_falsy_string(),
                ]),
                "uppercase-string" | "non-empty-uppercase-string" => PhpType::intersection(vec![
                    PhpType::uppercase_string(),
                    PhpType::non_falsy_string(),
                ]),
                "literal-string" | "non-empty-literal-string" => PhpType::intersection(vec![
                    PhpType::literal_string(),
                    PhpType::non_falsy_string(),
                ]),
                _ => self.clone(),
            }),
            _ => Some(self.clone()),
        }
    }

    /// Return the part of a type that can be falsy.
    ///
    /// The mirror of [`Self::truthy_type`], for the branch an `if ($x)`
    /// skips rather than the one it enters. Members that are *always*
    /// truthy are dropped and the rest keep only their falsy values: `bool`
    /// becomes `false`, `int` becomes `0`, `string` becomes `''|'0'`, an
    /// array that may be empty becomes `array{}`, and `mixed` becomes the
    /// fixed set of every falsy value.  `None` comes back when nothing the
    /// type describes could have been falsy.
    pub fn falsy_type(&self) -> Option<PhpType> {
        let mut falsy = Vec::new();
        self.push_falsy_members(&mut falsy);
        match falsy.len() {
            0 => None,
            1 => falsy.pop(),
            _ => Some(PhpType::union(falsy)),
        }
    }

    /// Append this type's falsy members to `out`.
    ///
    /// The unions and nullables a type is built from are flattened on the
    /// way, so [`Self::union`] is handed one list of atoms and can
    /// deduplicate the `null` that every nullable member contributes:
    /// `?int|?string` is falsy as `int|null|string`, not as a union with
    /// the same `null` in it twice.
    fn push_falsy_members(&self, out: &mut Vec<PhpType>) {
        match self.kind() {
            // `null` is falsy, so it survives whatever the inner type does.
            TypeKind::Nullable(inner) => {
                inner.push_falsy_members(out);
                out.push(PhpType::null());
            }
            TypeKind::Union(members) => {
                for member in members.iter() {
                    member.push_falsy_members(out);
                }
            }
            _ if self.truthiness() == Some(true) => {}
            _ if self.is_bool() => out.push(PhpType::false_()),
            _ if self.is_mixed() => {
                out.extend([
                    PhpType::literal_int("0"),
                    PhpType::literal_float("0.0"),
                    PhpType::literal_string_value(""),
                    PhpType::literal_string_value("0"),
                    PhpType::array_shape(Vec::new()),
                    PhpType::false_(),
                    PhpType::null(),
                ]);
            }
            TypeKind::IntRange(min, max) => {
                let bound = |b: &Atom| b.parse::<i64>().ok();
                let above_min = bound(min).is_none_or(|min| min <= 0);
                let below_max = bound(max).is_none_or(|max| max >= 0);
                if above_min && below_max {
                    out.push(PhpType::literal_int("0"));
                }
            }
            TypeKind::Named(name) => with_keyword_lowercase(name, |lower| match lower {
                "int" | "integer" | "non-negative-int" | "non-positive-int" => {
                    out.push(PhpType::literal_int("0"));
                }
                "float" | "double" => out.push(PhpType::literal_float("0.0")),
                "string" => {
                    out.push(PhpType::literal_string_value(""));
                    out.push(PhpType::literal_string_value("0"));
                }
                "non-empty-string" | "numeric-string" => {
                    out.push(PhpType::literal_string_value("0"));
                }
                "numeric" => out.extend([
                    PhpType::literal_int("0"),
                    PhpType::literal_float("0.0"),
                    PhpType::literal_string_value("0"),
                ]),
                "array-key" => out.extend([
                    PhpType::literal_int("0"),
                    PhpType::literal_string_value(""),
                    PhpType::literal_string_value("0"),
                ]),
                "scalar" => out.extend([
                    PhpType::literal_int("0"),
                    PhpType::literal_float("0.0"),
                    PhpType::literal_string_value(""),
                    PhpType::literal_string_value("0"),
                    PhpType::false_(),
                ]),
                _ if self.is_array_like() => out.push(PhpType::array_shape(Vec::new())),
                _ => out.push(self.clone()),
            }),
            // `[]` is the only falsy array, whatever the array was declared
            // to hold.
            _ if self.is_array_like() => out.push(PhpType::array_shape(Vec::new())),
            _ => out.push(self.clone()),
        }
    }

    /// The non-empty counterpart of an array type: `array<K, V>` becomes
    /// `non-empty-array<K, V>` and `list<T>` becomes `non-empty-list<T>`.
    /// The `T[]` slice spelling is sugar for `array<T>` and refines the
    /// same way.
    ///
    /// Everything else comes back unchanged, including a shape (its own
    /// entries already say whether it can be empty) and a type that is
    /// non-empty by name already.
    pub fn non_empty_array_form(&self) -> PhpType {
        match self.kind() {
            TypeKind::Named(name) if name == "array" => PhpType::named(atom("non-empty-array")),
            TypeKind::Named(name) if name == "list" => PhpType::named(atom("non-empty-list")),
            TypeKind::Generic(generic) if generic.name == "array" => {
                PhpType::generic_atom(atom("non-empty-array"), generic.args.clone())
            }
            TypeKind::Generic(generic) if generic.name == "list" => {
                PhpType::generic_atom(atom("non-empty-list"), generic.args.clone())
            }
            TypeKind::Array(element) => {
                PhpType::generic_atom(atom("non-empty-array"), vec![element.clone()])
            }
            _ => self.clone(),
        }
    }

    /// Whether this type promises at least one entry.
    ///
    /// An optional entry (`array{a?: int}`) can be the only one there is,
    /// so a shape only counts once one of its entries is required, and a
    /// union only counts once every member does.
    pub fn is_provably_non_empty(&self) -> bool {
        match self.kind() {
            TypeKind::Named(name) => is_non_empty_array_name(name.as_str()),
            TypeKind::Generic(generic) => is_non_empty_array_name(generic.name.as_str()),
            TypeKind::ArrayShape(_) | TypeKind::ListShape(_) => self
                .shape_entries()
                .is_some_and(|entries| entries.iter().any(|entry| !entry.optional)),
            TypeKind::Union(members) => members.iter().all(PhpType::is_provably_non_empty),
            _ => false,
        }
    }

    /// The type left behind after `unset($var[$key])` (or an element unset
    /// off a nested array-access chain) removes one entry.
    ///
    /// `key` is the literal string key being removed, when known — `None`
    /// for a dynamic key (`unset($arr[$i])`). A non-array-like type (an
    /// `ArrayAccess` object routes through `offsetUnset` instead of real
    /// array mutation) comes back unchanged.
    ///
    /// A shape drops the matching entry outright when the key is known,
    /// since that key provably no longer exists. A dynamic key could have
    /// removed any one of the shape's entries, so every entry becomes
    /// optional instead — none of them is provably still there. Either way
    /// a shape that loses its only required entries is no longer provably
    /// non-empty, which is what lets a following `foreach` know its body
    /// might not run. Losing an entry also breaks a list's sequential-key
    /// promise, so a list shape is demoted to a plain array shape.
    ///
    /// A `non-empty-array`/`non-empty-list` (tracked by name, not by
    /// entries) loses that promise unconditionally: removing any one
    /// element could have emptied it out entirely. A `list<T>` that may
    /// have lost an entry leaves a gap in its keys, so it becomes
    /// `array<int, T>`.
    pub fn after_element_unset(&self, key: Option<&str>) -> PhpType {
        match self.kind() {
            TypeKind::Nullable(inner) => PhpType::nullable(inner.after_element_unset(key)),
            TypeKind::Union(members) => {
                PhpType::union(members.iter().map(|m| m.after_element_unset(key)).collect())
            }
            // A list has no string keys, so removing one leaves it as it was.
            TypeKind::Named(name) if key.is_some() && is_list_name(name.as_str()) => self.clone(),
            TypeKind::Generic(generic) if key.is_some() && is_list_name(generic.name.as_str()) => {
                self.clone()
            }
            TypeKind::Named(name) if name == "non-empty-array" => PhpType::named(atom("array")),
            TypeKind::Named(name) if is_list_name(name.as_str()) => {
                PhpType::generic_array(PhpType::int(), PhpType::mixed())
            }
            TypeKind::Generic(generic) if generic.name == "non-empty-array" => {
                PhpType::generic_atom(atom("array"), generic.args.clone())
            }
            TypeKind::Generic(generic) if is_list_name(generic.name.as_str()) => {
                PhpType::generic_array(
                    PhpType::int(),
                    generic.args.first().cloned().unwrap_or_else(PhpType::mixed),
                )
            }
            TypeKind::ArrayShape(entries) => {
                let updated: Vec<ShapeEntry> = match key {
                    Some(key) => entries
                        .iter()
                        .filter(|entry| entry.key.as_deref() != Some(key))
                        .cloned()
                        .collect(),
                    None => entries
                        .iter()
                        .cloned()
                        .map(|mut entry| {
                            entry.optional = true;
                            entry
                        })
                        .collect(),
                };
                PhpType::array_shape(updated)
            }
            _ => self.clone(),
        }
    }
}
