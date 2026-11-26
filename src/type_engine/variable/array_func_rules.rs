//! Return-type rules for the array-producing and element-extracting
//! standard library functions.
//!
//! The rules themselves live here, independent of where the call's
//! arguments come from.  Two callers reach a call expression from
//! opposite directions and both need the same answers:
//!
//! * the AST walker, when the call is an assignment right-hand side and
//!   a parsed `ArgumentList` is in hand (see
//!   [`super::raw_type_inference`]), and
//! * the text-driven call resolver, when the call appears inline (as a
//!   subject, an array-access base, or another call's argument) and only
//!   the raw argument source text is available (see
//!   `type_engine::call_resolution`).
//!
//! [`ArrayFuncArgs`] is the seam between the two: each caller answers
//! the handful of questions the rules ask about an argument, and the
//! rules stay in one place so a fix to `array_map`'s element type
//! reaches every consumer.

use crate::php_type::{
    LiteralValue, PhpType, ShapeEntry, TypeKind, canonical_int_key, is_array_like_name,
};

use super::{ARRAY_ELEMENT_FUNCS, ARRAY_PRESERVING_FUNCS};

mod range;

/// Argument access needed by the array-function return-type rules.
pub(in crate::type_engine) trait ArrayFuncArgs {
    /// The unflattened type of the argument at `index` (e.g.
    /// `list<User>`), or `None` when it cannot be resolved.
    fn arg_raw_type(&self, index: usize) -> Option<PhpType>;

    /// The argument at `index` when it is written as a `true` or `false`
    /// literal, `None` for every other expression (including one that
    /// merely *resolves* to a bool).
    fn bool_literal(&self, index: usize) -> Option<bool>;

    /// Whether an argument was written at `index`.
    ///
    /// Distinguishes an omitted argument from one that is present but
    /// unresolvable, which [`arg_raw_type`](Self::arg_raw_type) cannot:
    /// both come back as `None`. `array_filter($a)` and
    /// `array_filter($a, $cb)` differ only in this.
    fn has_arg(&self, index: usize) -> bool;

    /// Whether the argument at `index` is unpacked with `...`.
    ///
    /// A spread holds the values for *several* parameters rather than one,
    /// so its own type describes a different level of nesting than the rules
    /// expect: `array_merge(...$arrays)` passes a `list<list<T>>` where the
    /// rule reads a `list<T>`. A rule that walks the argument list has to
    /// decline as soon as it sees one.
    fn is_spread(&self, index: usize) -> bool;

    /// Whether the argument at `index` is written as `name: value`.
    ///
    /// The accessors here are positional, so a named argument may stand
    /// for a different parameter than its index suggests: in
    /// `number_format($n, 2, thousands_separator: '')` index 2 is the
    /// thousands separator, not the decimal one.
    fn is_named(&self, index: usize) -> bool;

    /// The return type the callback at `index` declares: the `: T` hint of
    /// an inline closure or arrow function, or the declared return of the
    /// function a callable string names (`'intval'`).  `None` when the
    /// argument is neither, or declares no return type.
    fn callback_declared_return_type(&self, index: usize) -> Option<PhpType>;

    /// The return type inferred from the body of the closure or arrow
    /// function at `index`, with its first parameter seeded to
    /// `param_type`.
    fn callback_inferred_return_type(&self, index: usize, param_type: &PhpType) -> Option<PhpType>;

    /// Whether `inferred` is a subtype of `declared`, following the class
    /// hierarchy rather than just the two types' structure.
    ///
    /// A closure's real return type is the intersection of what it declares
    /// and what its body produces (PHPStan's `intersectButNotNever`), which
    /// for a hierarchy pair is simply the narrower of the two.
    fn narrows(&self, inferred: &PhpType, declared: &PhpType) -> bool;

    /// The argument at `index` written as a bare constant name or integer
    /// literal (`ARRAY_FILTER_USE_KEY`, `2`), with any namespace prefix
    /// stripped.  `None` for any other expression.
    fn arg_atom_text(&self, index: usize) -> Option<String>;

    /// `subject` narrowed to the values that make the closure or arrow
    /// function at `index` accept them through its `param_index`th
    /// parameter.  `None` when the callback asserts nothing about it.
    fn callback_param_narrowing(
        &self,
        index: usize,
        param_index: usize,
        subject: &PhpType,
    ) -> Option<PhpType>;

    /// Split `subject` by the type-check function `guard` into the values
    /// it accepts (`None` when none pass) and whether it can reject any.
    /// `None` when `guard` is not a type check.
    fn guard_split(&self, guard: &str, subject: &PhpType) -> Option<(Option<PhpType>, bool)> {
        crate::type_engine::type_ops::condition_facts::split_type_by_guard_name(
            guard, subject, None,
        )
    }

    /// What reading the instance property `name` off an object of type
    /// `object` gives from where the call is written, or `None` when the
    /// object's class cannot be loaded.
    fn object_property(&self, object: &PhpType, name: &str) -> Option<ObjectProperty> {
        let _ = (object, name);
        None
    }
}

/// What [`ArrayFuncArgs::object_property`] finds, the way PHPStan's
/// `ArrayColumnHelper` reads an object row.
pub(in crate::type_engine) enum ObjectProperty {
    /// The property is there and readable, holding this.
    Readable(PhpType),
    /// No object of the class has a property the caller can read under
    /// that name: the class is final and lacks it (or hides it without
    /// magic to read it through).
    Missing,
    /// A subclass instance may have a readable property of this type
    /// (a `protected` one read from outside a non-final class).
    MaybeReadable(PhpType),
    /// Nothing is known: a non-final class may grow the property, a
    /// `private` one may be shadowed, or `__isset()`/`__get()` decide.
    Unknown,
}

/// A call's arguments with the one at `index` replaced by `ty`, so a rule
/// for one value can run over each entry of an array argument.
struct SwappedArg<'a> {
    args: &'a dyn ArrayFuncArgs,
    index: usize,
    ty: PhpType,
}

impl ArrayFuncArgs for SwappedArg<'_> {
    fn arg_raw_type(&self, index: usize) -> Option<PhpType> {
        if index == self.index {
            Some(self.ty.clone())
        } else {
            self.args.arg_raw_type(index)
        }
    }
    fn bool_literal(&self, index: usize) -> Option<bool> {
        (index != self.index)
            .then(|| self.args.bool_literal(index))
            .flatten()
    }
    fn has_arg(&self, index: usize) -> bool {
        self.args.has_arg(index)
    }
    fn is_spread(&self, index: usize) -> bool {
        self.args.is_spread(index)
    }
    fn is_named(&self, index: usize) -> bool {
        self.args.is_named(index)
    }
    fn callback_declared_return_type(&self, index: usize) -> Option<PhpType> {
        self.args.callback_declared_return_type(index)
    }
    fn callback_inferred_return_type(&self, index: usize, param_type: &PhpType) -> Option<PhpType> {
        self.args.callback_inferred_return_type(index, param_type)
    }
    fn narrows(&self, inferred: &PhpType, declared: &PhpType) -> bool {
        self.args.narrows(inferred, declared)
    }
    fn object_property(&self, object: &PhpType, name: &str) -> Option<ObjectProperty> {
        self.args.object_property(object, name)
    }
    fn arg_atom_text(&self, index: usize) -> Option<String> {
        (index != self.index)
            .then(|| self.args.arg_atom_text(index))
            .flatten()
    }
    fn callback_param_narrowing(
        &self,
        index: usize,
        param_index: usize,
        subject: &PhpType,
    ) -> Option<PhpType> {
        self.args
            .callback_param_narrowing(index, param_index, subject)
    }
}

/// For known array-producing functions, resolve the **raw output type**
/// (e.g. `list<User>`) from the input arguments.
///
/// Element-extracting functions are handled by
/// [`array_func_element_type`], which the caller consults first.
pub(in crate::type_engine) fn array_func_raw_type(
    func_name: &str,
    args: &dyn ArrayFuncArgs,
) -> Option<PhpType> {
    // A fully-qualified call (`\array_filter($a)`) carries the leading
    // separator in its identifier text, while every table below is keyed on
    // the bare name.  Normalising here covers both the AST and the
    // text-driven caller at once.
    let func_name = func_name.trim_start_matches('\\');

    // `array_merge` appends every argument instead of rearranging one, so
    // the result describes all of them together rather than just the first.
    if func_name.eq_ignore_ascii_case("array_merge") {
        return array_merge_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_fill")
        && let Some(filled) = array_fill_type(args)
    {
        return Some(filled);
    }

    if func_name.eq_ignore_ascii_case("array_keys")
        && let Some(keys) = shape_keys_type(args)
    {
        return Some(keys);
    }

    if ((func_name.eq_ignore_ascii_case("array_keys") && !args.has_arg(1))
        || func_name.eq_ignore_ascii_case("array_values"))
        && let Some(list) = keys_or_values_list(args, func_name.eq_ignore_ascii_case("array_keys"))
    {
        return Some(list);
    }

    if func_name.eq_ignore_ascii_case("array_column") {
        return array_column_type(args);
    }

    if func_name.eq_ignore_ascii_case("str_split") || func_name.eq_ignore_ascii_case("mb_str_split")
    {
        return str_split_type(args, func_name.eq_ignore_ascii_case("mb_str_split"));
    }

    if func_name.eq_ignore_ascii_case("array_reverse") {
        return array_reverse_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_slice") {
        return array_slice_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_count_values") {
        return array_count_values_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_change_key_case") {
        return array_change_key_case_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_pad") {
        return array_pad_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_intersect_key")
        && let Some(intersected) = array_intersect_key_type(args)
    {
        return Some(intersected);
    }

    if func_name.eq_ignore_ascii_case("array_replace") {
        return array_replace_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_combine") {
        return array_combine_type(args);
    }

    if func_name.eq_ignore_ascii_case("array_fill_keys")
        && let Some(filled) = array_fill_keys_type(args)
    {
        return Some(filled);
    }

    // Handed an array subject, the `preg_` replacers return its keys with
    // string values, dropping an entry whose replacement fails (and, for
    // `preg_filter`, one the pattern does not match). So a shape keeps its
    // keys, each of them optional.
    let preg_subject = if ["preg_replace", "preg_replace_callback", "preg_filter"]
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        Some(2)
    } else if func_name.eq_ignore_ascii_case("preg_replace_callback_array") {
        Some(1)
    } else {
        None
    };
    // `str_replace()` handed a shape replaces in each entry, which then
    // folds the way a string subject does.
    if ["str_replace", "str_ireplace"]
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
        && let Some(entries) = args
            .arg_raw_type(2)
            .as_ref()
            .and_then(explicit_key_shape_entries)
    {
        return Some(PhpType::array_shape(
            entries
                .into_iter()
                .map(|entry| {
                    let swapped = SwappedArg {
                        args,
                        index: 2,
                        ty: entry.value_type.clone(),
                    };
                    let value_type = entry
                        .value_type
                        .is_string_subtype()
                        .then(|| {
                            super::string_func_rules::string_func_return_type(func_name, &swapped)
                        })
                        .flatten()
                        .unwrap_or_else(PhpType::string);
                    ShapeEntry {
                        value_type,
                        ..entry
                    }
                })
                .collect(),
        ));
    }

    if let Some(subject) = preg_subject
        && let Some(entries) = args
            .arg_raw_type(subject)
            .as_ref()
            .and_then(explicit_key_shape_entries)
    {
        return Some(PhpType::array_shape(
            entries
                .into_iter()
                .map(|entry| ShapeEntry {
                    value_type: PhpType::string(),
                    optional: true,
                    ..entry
                })
                .collect(),
        ));
    }

    // A shape the filter can decide entry by entry keeps its keys and its
    // per-entry types instead of collapsing into the container it describes.
    if func_name.eq_ignore_ascii_case("array_filter")
        && let Some(filtered) = filter_shape_type(args)
    {
        return Some(filtered);
    }

    // Type-preserving functions: output array has same element type.
    if ARRAY_PRESERVING_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        // Every one of these rearranges the array (reorders, renumbers,
        // drops or chunks entries), so a constant shape does not survive
        // the call and is generalized to the container it describes.
        let raw = args.arg_raw_type(0)?.generalized_array();
        // Only a parameterised iterable carries an element type worth
        // preserving; a bare `array`/`iterable` is a `Named` kind with no
        // value argument to extract, so the rule declines and the stub's
        // own return type stands.
        //
        // `skip_scalar` must stay off here. It asks "is the element
        // non-scalar", which is not the question: a `list<string>` is
        // every bit as worth preserving as a `list<User>`, and answering
        // it with `true` silently dropped every scalar-element array back
        // to a bare `array`.
        if raw.extract_value_type(false).is_some() {
            // `array_filter($a)` with no callback keeps exactly the
            // members that survive a truthiness test, so the element type
            // drops `null`, `false`, `0`, `''` and friends. With a
            // callback the kept members are whatever it approves of, which
            // says nothing about their type.
            if func_name.eq_ignore_ascii_case("array_filter") {
                let raw = filtered_container(&raw);
                if !args.has_arg(1) {
                    return Some(filter_element_type(&raw).unwrap_or(raw));
                }
                // A callback decides which entries survive, so what it
                // asserts about the value or the key it is handed
                // describes the result.
                return Some(filter_callback_type(&raw, args).unwrap_or(raw));
            }
            // Dropping duplicates keeps the first of each, under its key, so
            // a list's keys may have gaps after it.
            if func_name.eq_ignore_ascii_case("array_unique")
                && let Some((form, _, value)) = generic_array_parts(&raw)
                && form.is_list()
            {
                let array = PhpType::generic_array(PhpType::int_range("0", "max"), value);
                return Some(if form.is_non_empty() {
                    array.non_empty_array_form()
                } else {
                    array
                });
            }
            // Comparing with a second array can drop any entry, the first
            // one included: the result may be empty, and a list's keys
            // may have gaps.
            let lower = func_name.to_ascii_lowercase();
            let compares = [
                "array_diff",
                "array_udiff",
                "array_intersect",
                "array_uintersect",
            ]
            .iter()
            .any(|prefix| lower.starts_with(prefix));
            if compares
                && args.has_arg(1)
                && let Some((form, key, value)) = generic_array_parts(&raw)
            {
                return Some(if form.is_list() {
                    PhpType::generic_array(PhpType::int_range("0", "max"), value)
                } else {
                    PhpType::generic(form.name(false), key.into_iter().chain([value]).collect())
                });
            }
            return Some(raw);
        }
    }

    if func_name.eq_ignore_ascii_case("array_chunk") {
        return array_chunk_type(args);
    }

    // `range` builds its elements from the bounds rather than from an array
    // it was handed, and a single fractional bound makes the whole range
    // fractional. That is a question about every argument at once, which the
    // conditional return type in `stub_patches` cannot ask.
    if func_name.eq_ignore_ascii_case("range") {
        return range::range_type(args);
    }

    // Splitting a string always leaves at least one piece, so the pointer
    // idiom `end(explode('/', $path))` is a `string`.  Only a negative
    // `$limit`, which drops pieces from the end, can leave none; an empty
    // separator throws rather than returning.  Mirrors PHPStan's
    // `ExplodeFunctionDynamicReturnTypeExtension`.
    if func_name.eq_ignore_ascii_case("explode") {
        let keeps_all = !args.has_arg(2)
            || args
                .arg_raw_type(2)
                .is_some_and(|limit| limit.is_subtype_of(&PhpType::parse("int<0, max>")));
        let pieces = PhpType::string();
        return Some(if keeps_all {
            PhpType::generic("non-empty-list", vec![pieces])
        } else {
            PhpType::list(pieces)
        });
    }

    // array_map: callback is first arg, array is second.
    // The callback's return type determines the output element type.
    if func_name.eq_ignore_ascii_case("array_map") {
        if let Some(mapped) = array_map_shape(args) {
            return Some(mapped);
        }
        let seed = args.arg_raw_type(1).and_then(|t| t.iterable_element_type());
        let element = array_map_value(array_map_element_type(args, seed)?);
        return Some(array_map_container(element, args));
    }

    // iterator_to_array: converts an iterator to an array, preserving
    // key and value types.  `iterator_to_array($iter)` where `$iter`
    // is `Iterator<int, Foo>` produces `array<int, Foo>`.  When only
    // a value type is available (single generic param), produces
    // `list<Foo>`.
    if func_name.eq_ignore_ascii_case("iterator_to_array") {
        let raw = args.arg_raw_type(0)?;
        let val = raw
            .iterable_element_type()
            .map(|value| value.widen_scalar_literals());
        // `preserve_keys: false` renumbers the result, so the key type
        // the iterator declared no longer describes it.
        if args.bool_literal(1) == Some(false) {
            return Some(val.map_or_else(PhpType::array, PhpType::list));
        }
        let key = raw
            .iterable_key_type()
            .and_then(|key| super::array_shape_writes::normalize_array_key_type(&key));
        return match (key, val) {
            (Some(k), Some(v)) => Some(PhpType::generic_array(k, v)),
            (None, Some(v)) => Some(PhpType::list(v)),
            _ => Some(PhpType::array()),
        };
    }

    None
}

/// For known array functions, resolve the **element type**
/// (e.g. `User`) of the output.
///
/// This only covers true element-extracting functions (`array_pop`,
/// `current`, …) that return a single element.  Array-producing
/// functions like `array_map` and `iterator_to_array` are handled
/// exclusively by [`array_func_raw_type`], which preserves the
/// container type (e.g. `list<User>`).  Returning the element type here
/// would lose the array wrapper and break downstream consumers that
/// need to walk bracket segments (e.g. `$result[0]->`).
pub(in crate::type_engine) fn array_func_element_type(
    func_name: &str,
    args: &dyn ArrayFuncArgs,
) -> Option<PhpType> {
    // See the matching note in [`array_func_raw_type`]: `\array_pop($a)`
    // reaches here spelled with its leading separator.
    let func_name = func_name.trim_start_matches('\\');

    if func_name.eq_ignore_ascii_case("array_find")
        && let Some(found) = array_find_type(args)
    {
        return Some(found);
    }

    if ARRAY_ELEMENT_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        // A scalar element is the honest answer for `array_pop(list<string>)`
        // just as `User` is for `list<User>`, so the element type is read
        // without `skip_scalar`.
        let raw = args.arg_raw_type(0)?;
        return element_or_sentinel(func_name, &raw);
    }

    // `array_sum`/`array_product` are declared `int|float` because the
    // result follows PHP's numeric promotion, but an all-`int` array can
    // only sum to an `int`. The element type decides it, which is why this
    // cannot be expressed as a `@template` on the stub: `array<TValue>` with
    // `@return TValue` would answer `string` for `array_sum(list<string>)`
    // rather than the `int|float` PHP actually produces.
    // `max`/`min` hand back one of the values they were given, which the
    // stub can only spell as `mixed`.
    if func_name.eq_ignore_ascii_case("max") || func_name.eq_ignore_ascii_case("min") {
        return min_max_type(args, func_name.eq_ignore_ascii_case("max"));
    }

    if func_name.eq_ignore_ascii_case("count") || func_name.eq_ignore_ascii_case("sizeof") {
        return count_type(args);
    }

    // The key readers are stubbed `TKey|null` because an empty array has no
    // key to report. An argument that proves it has entries rules that out.
    if func_name.eq_ignore_ascii_case("array_key_first")
        || func_name.eq_ignore_ascii_case("array_key_last")
        || func_name.eq_ignore_ascii_case("key")
    {
        let raw = args.arg_raw_type(0)?;
        if !func_name.eq_ignore_ascii_case("key")
            && let Some(key) =
                shape_end_key_type(&raw, func_name.eq_ignore_ascii_case("array_key_first"))
        {
            return Some(key);
        }
        return raw
            .is_provably_non_empty()
            .then(|| array_key_domain(&raw))
            .flatten();
    }

    let product = func_name.eq_ignore_ascii_case("array_product");
    if product || func_name.eq_ignore_ascii_case("array_sum") {
        return array_arithmetic_type(&args.arg_raw_type(0)?, product);
    }

    None
}

/// How many alternatives an `array_sum()`/`array_product()` total may carry
/// while a shape is worked through entry by entry.
const MAX_TOTAL_ALTERNATIVES: usize = 32;

/// `array_sum()` / `array_product()`: the total of the array's values, worked
/// out with the arithmetic `+` / `*` performs, so the declared `int|float`
/// narrows as far as the values decide it. `None` for an array whose values
/// say nothing the declared type does not.
fn array_arithmetic_type(raw: &PhpType, product: bool) -> Option<PhpType> {
    // An array nobody typed the elements of decides neither half, and the
    // sum is then exactly what adding two `mixed` values is: the pair,
    // benevolent, because it is a gap in what the code said rather than a
    // value measured to be two things. Enforcing both halves reported
    // `array_sum($bytes)` handed to an `int` parameter as a mismatch on code
    // that is fine. PHPStan reaches the same answer by summing the element
    // type with itself.
    let undecided = || {
        Some(PhpType::benevolent(PhpType::union(vec![
            PhpType::int(),
            PhpType::float(),
        ])))
    };
    let mut results: Vec<PhpType> = Vec::new();
    for array in raw.union_members() {
        let total = if matches!(
            array.kind(),
            TypeKind::ArrayShape(_) | TypeKind::ListShape(_)
        ) {
            shape_total(array, product)?
        } else {
            let Some(element) = array.iterable_element_type() else {
                return undecided();
            };
            if element.union_members().iter().any(|m| m.is_mixed()) {
                return undecided();
            }
            element_total(&element, array.is_provably_non_empty(), product)?
        };
        if !results.contains(&total) {
            results.push(total);
        }
    }
    (!results.is_empty()).then(|| PhpType::union(results))
}

/// The total of a shape, each entry combined with the running total one
/// alternative at a time (so `array{1, 2|3.4}` sums to `3|4.4`), an optional
/// entry counted both present and absent.
fn shape_total(shape: &PhpType, product: bool) -> Option<PhpType> {
    use super::rhs_resolution::{ArithmeticOpKind, infer_arithmetic_result_type};
    use crate::types::ResolvedType;

    let (op, identity) = if product {
        (ArithmeticOpKind::Multiplication, "1")
    } else {
        (ArithmeticOpKind::Addition, "0")
    };
    let mut totals = vec![PhpType::literal_int(identity)];
    for entry in shape.shape_entries()? {
        let mut next: Vec<PhpType> = Vec::new();
        if entry.optional {
            next.clone_from(&totals);
        }
        for value in entry.value_type.union_members() {
            let value = [ResolvedType::from_type_string(value.clone())];
            for total in &totals {
                let total = [ResolvedType::from_type_string(total.clone())];
                let combined = infer_arithmetic_result_type(&total, &value, op);
                for member in combined.union_members() {
                    if !next.contains(member) {
                        next.push(member.clone());
                    }
                }
            }
        }
        if next.len() > MAX_TOTAL_ALTERNATIVES {
            return None;
        }
        totals = next;
    }
    Some(PhpType::union(totals))
}

/// The total of an array known only by its element type.
///
/// A sum of integers stays on the side of zero its elements are on: adding
/// any number of `int<1, max>` gives `int<0, max>`, or `int<1, max>` when
/// there is at least one. An array that may be empty sums to `0`, which an
/// all-`float` array otherwise never produces. A product of integers is an
/// integer and a product of floats a float.
fn element_total(element: &PhpType, non_empty: bool, product: bool) -> Option<PhpType> {
    use crate::type_engine::type_ops::int_interval::IntInterval;

    let mut ints: Vec<IntInterval> = Vec::new();
    let mut has_float = false;
    for member in element.union_members() {
        if member.is_float() || member.is_float_subtype() {
            has_float = true;
        } else {
            ints.push(IntInterval::of_operand(member)?);
        }
    }
    let mut parts: Vec<PhpType> = Vec::new();
    if product {
        if has_float && !ints.is_empty() {
            return None;
        }
        parts.push(if has_float {
            PhpType::float()
        } else {
            PhpType::int()
        });
        return Some(PhpType::union(parts));
    }
    if ints.is_empty() {
        if !non_empty {
            parts.push(PhpType::literal_int("0"));
        }
    } else {
        let hull = IntInterval::hull_of(&ints);
        let edge = |bound: Option<i64>, toward_zero: bool| match bound {
            Some(bound) if toward_zero => Some(if non_empty { bound } else { 0 }),
            _ => None,
        };
        parts.push(
            IntInterval {
                min: edge(hull.min, hull.min.is_some_and(|min| min >= 0)),
                max: edge(hull.max, hull.max.is_some_and(|max| max <= 0)),
            }
            .into_type(),
        );
    }
    if has_float {
        parts.push(PhpType::float());
    }
    Some(PhpType::union(parts))
}

/// What an element function hands back for `raw`: an element, or the
/// `false` / `null` it returns when there is no element to give.
///
/// The pointer readers answer `false` and the removers and finders answer
/// `null`.  `reset()`, `end()`, `current()` and the removers only miss on an
/// empty array, so an argument that proves it has entries rules the sentinel
/// out; `next()` and `prev()` can step off either end of any array, and
/// `array_find()` can match nothing, so they always keep it.
fn element_or_sentinel(func_name: &str, raw: &PhpType) -> Option<PhpType> {
    let (sentinel, always) = match func_name.to_ascii_lowercase().as_str() {
        "reset" | "end" | "current" => (PhpType::false_(), false),
        "next" | "prev" => (PhpType::false_(), true),
        "array_find" => (PhpType::null(), true),
        _ => (PhpType::null(), false),
    };
    if matches!(raw.kind(), TypeKind::ArrayShape(_) | TypeKind::ListShape(_))
        && raw
            .shape_entries()
            .is_some_and(|entries| entries.is_empty())
    {
        return Some(sentinel);
    }
    // A remover takes the entry at its end of the shape: the first one
    // certainly present, or any optional one before it.
    let from_front = if func_name.eq_ignore_ascii_case("array_shift") {
        Some(true)
    } else if func_name.eq_ignore_ascii_case("array_pop") {
        Some(false)
    } else {
        None
    };
    if let Some(from_front) = from_front
        && let TypeKind::ArrayShape(entries) = raw.kind()
    {
        let (mut values, present) = if from_front {
            leading_entry_values(entries.iter())
        } else {
            leading_entry_values(entries.iter().rev())
        };
        if !present {
            values.push(sentinel);
        }
        return Some(PhpType::join_runtime_value_types(values));
    }
    let element = raw.iterable_element_type()?;
    if element.is_mixed() || (!always && raw.is_provably_non_empty()) {
        return Some(element);
    }
    let mut members: Vec<PhpType> = element.union_members().into_iter().cloned().collect();
    members.push(sentinel);
    Some(PhpType::union(members))
}

/// What `array_find()` returns when its callback narrows the value it is
/// handed: the accepted part of each element, or `null` when nothing
/// matches. `None` when the callback says nothing about the value.
///
/// Over a shape whose entries a type-check callback (`'is_int'`) decides one
/// by one, the search stops at the first entry certainly accepted, so later
/// entries cannot be found and the result is not `null`.
fn array_find_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let raw = args.arg_raw_type(0)?;
    if let Some(entries) = explicit_key_shape_entries(&raw)
        && let Some(guards) = callback_guard_names(args, 1)
    {
        let mut values: Vec<PhpType> = Vec::new();
        let mut found = false;
        for entry in &entries {
            let mut certain = !entry.optional;
            for guard in &guards {
                let (accepted, may_reject) = args.guard_split(guard, &entry.value_type)?;
                certain &= accepted.is_some() && !may_reject;
                if let Some(accepted) = accepted
                    && !values.contains(&accepted)
                {
                    values.push(accepted);
                }
            }
            if certain {
                found = true;
                break;
            }
        }
        if !found {
            values.push(PhpType::null());
        }
        return Some(PhpType::join_runtime_value_types(values));
    }
    let element = raw.iterable_element_type()?;
    let narrowed = filter_callback_narrowing(args, 0, &element)?;
    let mut members: Vec<PhpType> = narrowed.union_members().into_iter().cloned().collect();
    members.push(PhpType::null());
    Some(PhpType::union(members))
}

/// The values of `entries` up to and including the first one that is
/// certainly present, and whether there was one.
fn leading_entry_values<'a>(entries: impl Iterator<Item = &'a ShapeEntry>) -> (Vec<PhpType>, bool) {
    let mut values: Vec<PhpType> = Vec::new();
    for entry in entries {
        values.push(entry.value_type.clone());
        if !entry.optional {
            return (values, true);
        }
    }
    (values, false)
}

/// The function a callable string names (`'intval'`, `"\\strlen"`), with
/// any leading namespace separator stripped.
///
/// PHP resolves a callable string against the global namespace, ignoring
/// the calling file's `use` table, so the name needs no further
/// qualification. Returns `None` for anything that is not a plain quoted
/// name: a `'Foo::bar'` string or a `[$obj, 'method']` pair names a method,
/// which the callers here do not resolve.
pub(in crate::type_engine) fn callable_string_function_name(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    let inner = trimmed
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .or_else(|| {
            trimmed
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })?;
    let name = inner.trim_start_matches('\\');
    let is_name = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\\');
    is_name.then_some(name)
}

/// The type `array_merge` builds from its arguments.
///
/// PHP appends each argument in turn, so the values are the union of every
/// argument's. This is the whole point of the rule: `array_merge` is the
/// one member of the array family that concatenates rather than rearranges,
/// and reading only the first argument left the accumulator idiom
/// (`$out = []; … $out = array_merge($out, $more);`) permanently typed as
/// the empty array it started as.
///
/// An empty array shape adds neither a key nor a value and is skipped,
/// which is what lets `array_merge([], $items)` answer `list<Item>`.  Any
/// other argument that cannot be typed could contribute anything, so the
/// rule declines rather than claim a union that is missing a member.
///
/// The keys follow two different rules: an integer key is renumbered as the
/// entry is appended, while a string key is carried over (a later argument
/// overwriting an earlier one). So the result is a `list` exactly when
/// every argument promises integer keys, and keeps only the string half
/// when none of them does.
///
/// An argument that names just its value type (`array<T>`, `T[]`, bare
/// `array`) promises nothing about its keys, and neither can the result:
/// those merge to `array<V>`, the same open domain that went in. Spelling
/// the domain out as `int|string` instead would be the honest reading of
/// what PHP allows, but it collides with every consumer that reads the
/// implicit `int` of a `T[]`, and turns a `T[]` a signature accepts today
/// into an argument it rejects.
fn array_merge_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if let Some(merged) = merge_shapes(args, true) {
        return Some(merged);
    }
    let int_ty = PhpType::int();
    let mut values: Vec<PhpType> = Vec::new();
    let mut keys: Vec<PhpType> = Vec::new();
    let mut open_keys = false;
    let mut index = 0;
    while args.has_arg(index) {
        let mut raw = args.arg_raw_type(index)?;
        // A spread passes each of its elements as an array to merge, and
        // any number of them, so it contributes what one element would.
        if args.is_spread(index) {
            raw = raw.iterable_element_type()?;
        }
        if raw.is_empty_array_shape() {
            index += 1;
            continue;
        }
        let container = raw.generalized_array();
        values.push(container.iterable_element_type()?);
        match array_key_domain(&container) {
            Some(key) => keys.push(key),
            None => open_keys = true,
        }
        index += 1;
    }

    // Every argument was an empty shape, so there is nothing to name.
    if values.is_empty() {
        return None;
    }
    let value = PhpType::join_runtime_value_types(values);
    if open_keys {
        return Some(PhpType::generic_array_val(value));
    }
    let key = PhpType::join_runtime_value_types(keys);
    if key.is_subtype_of(&int_ty) {
        return Some(PhpType::list(value));
    }
    Some(PhpType::generic_array(key, value))
}

/// `array_intersect_key($array, ...$arrays)`: the entries of `$array` whose
/// key every other array also has.
///
/// A shape keeps each entry whose key the others may have, optional unless
/// every other is a shape that certainly has it. A generic array keeps its
/// values under the keys the arrays' key types share: none in common gives
/// `array{}`, and a list intersected with lists stays one. Only a single
/// array is returned unchanged, so a non-empty one may come back empty.
fn array_intersect_key_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let mut arrays: Vec<(PhpType, bool)> = Vec::new();
    let mut index = 0;
    while args.has_arg(index) {
        if args.is_named(index) {
            return None;
        }
        let raw = args.arg_raw_type(index)?;
        if args.is_spread(index) {
            arrays.push((raw.iterable_element_type()?, true));
        } else {
            arrays.push((raw, false));
        }
        index += 1;
    }
    let ((first, _), others) = arrays.split_first()?;
    // A lone array, or the arrays one spread holds, keep the values and
    // key type of one of them.
    if others.is_empty() {
        return Some(first.clone());
    }
    if let TypeKind::ArrayShape(_) = first.kind() {
        let entries = explicit_key_shape_entries(first)?;
        let mut kept: Vec<ShapeEntry> = Vec::new();
        'entries: for mut entry in entries {
            let key = entry.key.as_deref()?;
            let key_type = crate::php_type::shape_key_literal(key)?;
            for (other, spread) in others {
                if other.is_empty_array_shape() {
                    continue 'entries;
                }
                match other.kind() {
                    TypeKind::ArrayShape(_) if !spread => match other.shape_entry(key) {
                        Some(found) => entry.optional |= found.optional,
                        None => continue 'entries,
                    },
                    _ => {
                        let domain = array_key_domain(&other.generalized_array());
                        if domain.is_some_and(|d| !key_type.is_subtype_of(&d)) {
                            continue 'entries;
                        }
                        entry.optional = true;
                    }
                }
            }
            kept.push(entry);
        }
        return Some(shape_with_implicit_keys(kept));
    }
    let container = first.generalized_array();
    let (form, _, value) = generic_array_parts(&container)?;
    let open_key = PhpType::union(vec![PhpType::int(), PhpType::string()]);
    let written_key = array_key_domain(&container);
    let mut key = written_key.clone().unwrap_or_else(|| open_key.clone());
    let mut all_lists = form.is_list();
    for (other, spread) in others {
        if other.is_empty_array_shape() {
            return Some(PhpType::array_shape(Vec::new()));
        }
        let generalized = other.generalized_array();
        all_lists &= generic_array_parts(&generalized).is_some_and(|(form, _, _)| form.is_list());
        // A shape's own keys, rather than the type they widen to.
        let domain = match other.kind() {
            TypeKind::ArrayShape(_) if !spread => {
                shape_key_alternatives(other).and_then(|alternatives| {
                    literal_key_union(alternatives.into_iter().flatten().map(|(key, _)| key))
                })
            }
            _ => array_key_domain(&generalized),
        };
        let Some(domain) = domain.and_then(|domain| {
            super::array_shape_writes::normalize_array_key_type_keeping_literals(&domain)
        }) else {
            continue;
        };
        key = match intersect_key_types(&key, &domain) {
            Some(key) => key,
            None => return Some(PhpType::array_shape(Vec::new())),
        };
    }
    if all_lists {
        return Some(PhpType::list(value));
    }
    // An array that never named its keys still does not, unless another
    // array narrowed them.
    if written_key.is_none() && key == open_key {
        return Some(PhpType::generic_array_val(value));
    }
    Some(PhpType::generic_array(key, value))
}

/// The keys two key types share, member by member, or `None` when they
/// share none.
fn intersect_key_types(a: &PhpType, b: &PhpType) -> Option<PhpType> {
    let mut shared: Vec<PhpType> = Vec::new();
    for x in a.union_members() {
        for y in b.union_members() {
            let common = if y.is_subtype_of(x) {
                y
            } else if x.is_subtype_of(y) {
                x
            } else {
                continue;
            };
            if !shared.contains(common) {
                shared.push(common.clone());
            }
        }
    }
    (!shared.is_empty()).then(|| PhpType::join_runtime_value_types(shared))
}

/// `array_replace()`: every argument's entries, a later one replacing an
/// earlier one of the same key. Unlike `array_merge()` no key is
/// renumbered, so a list stays a list only when every argument is one.
fn array_replace_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if let Some(replaced) = merge_shapes(args, false) {
        return Some(replaced);
    }
    let mut values: Vec<PhpType> = Vec::new();
    let mut keys: Vec<PhpType> = Vec::new();
    let mut open_keys = false;
    let mut all_lists = true;
    let mut non_empty = false;
    let mut index = 0;
    while args.has_arg(index) {
        let mut raw = args.arg_raw_type(index)?;
        if args.is_spread(index) {
            raw = raw.iterable_element_type()?;
        } else {
            non_empty |= raw.is_provably_non_empty();
        }
        index += 1;
        if raw.is_empty_array_shape() {
            continue;
        }
        let container = raw.generalized_array();
        all_lists &= generic_array_parts(&container).is_some_and(|(form, _, _)| form.is_list());
        values.push(container.iterable_element_type()?);
        match array_key_domain(&container) {
            Some(key) => keys.push(key),
            None => open_keys = true,
        }
    }
    if values.is_empty() {
        return None;
    }
    let value = PhpType::join_runtime_value_types(values);
    let result = if all_lists {
        PhpType::list(value)
    } else if open_keys {
        PhpType::generic_array_val(value)
    } else {
        PhpType::generic_array(PhpType::join_runtime_value_types(keys), value)
    };
    Some(if non_empty {
        result.non_empty_array_form()
    } else {
        result
    })
}

/// `array_merge` over arguments that are all array shapes, entry by entry.
///
/// A string key overwrites the entry an earlier argument gave it, in that
/// entry's position, and an integer key is appended under the next index.
/// An optional string key may or may not overwrite, so the value joins
/// both; an optional integer entry leaves every later index uncertain, and
/// the rule declines, as it does for any argument that is not a shape.
///
/// A spread of a shape whose entries are all present passes each entry as
/// an argument of its own. An argument that may be one of several shapes is
/// merged as the single shape they join into, a key only some of them have
/// becoming optional.
///
/// Without `renumber` (`array_replace`), an integer key overwrites the way
/// a string key does.
fn merge_shapes(args: &dyn ArrayFuncArgs, renumber: bool) -> Option<PhpType> {
    let mut arrays: Vec<PhpType> = Vec::new();
    let mut index = 0;
    while args.has_arg(index) {
        let raw = args.arg_raw_type(index)?;
        if !args.is_spread(index) {
            arrays.push(raw);
        } else {
            let TypeKind::ArrayShape(entries) = raw.kind() else {
                return None;
            };
            if entries.iter().any(|entry| entry.optional) {
                return None;
            }
            arrays.extend(entries.iter().map(|entry| entry.value_type.clone()));
        }
        index += 1;
    }

    let mut merged: Vec<ShapeEntry> = Vec::new();
    let mut next_index: i64 = 0;
    for array in arrays {
        let array = match array.kind() {
            TypeKind::Union(members) => {
                let (first, rest) = members.split_first()?;
                rest.iter()
                    .try_fold(first.clone(), |joined, member| joined.join_shapes(member))?
            }
            _ => array,
        };
        for entry in explicit_key_shape_entries(&array)? {
            let key = entry.key.as_deref().unwrap_or_default();
            if renumber && canonical_int_key(key).is_some() {
                if entry.optional {
                    return None;
                }
                merged.push(ShapeEntry {
                    key: Some(next_index.to_string()),
                    ..entry
                });
                next_index += 1;
            } else if let Some(existing) = merged
                .iter_mut()
                .find(|existing| existing.key.as_deref() == Some(key))
            {
                if entry.optional {
                    existing.value_type = PhpType::join_runtime_value_types(vec![
                        existing.value_type.clone(),
                        entry.value_type,
                    ]);
                } else {
                    existing.value_type = entry.value_type;
                    existing.optional = false;
                }
            } else {
                merged.push(entry);
            }
        }
    }
    Some(shape_with_implicit_keys(merged))
}

/// A single array shape's entries with every key spelled out, the
/// positional ones numbered the way PHP numbers them (one past the
/// largest integer key so far).
///
/// `None` for anything but a single shape: a union of shapes, a nullable
/// one, or a generic array has no one entry list to walk.
fn explicit_key_shape_entries(ty: &PhpType) -> Option<Vec<ShapeEntry>> {
    let TypeKind::ArrayShape(entries) = ty.kind() else {
        return None;
    };
    let mut next_index: i64 = 0;
    Some(
        entries
            .iter()
            .map(|entry| {
                let key = match &entry.key {
                    Some(key) => {
                        if let Some(n) = canonical_int_key(key) {
                            next_index = next_index.max(n + 1);
                        }
                        key.clone()
                    }
                    None => {
                        let key = next_index.to_string();
                        next_index += 1;
                        key
                    }
                };
                ShapeEntry {
                    key: Some(key),
                    ..entry.clone()
                }
            })
            .collect(),
    )
}

/// How many distinct keys a key union read off a shape may carry.
///
/// A literal array with more entries than this is a data table rather than
/// a set of cases a caller tells apart, and the union's pairwise absorption
/// is quadratic in its member count.
const MAX_KEY_ALTERNATIVES: usize = 32;

/// The keys of every alternative of an array-shape type, each as the
/// literal PHP stores it and paired with whether its entry is optional.
///
/// `None` unless every alternative is a single array shape whose keys can be
/// spelled as literals. A class-constant key other than `Foo::class` only
/// records the constant's name, not the value it stands for.
fn shape_key_alternatives(ty: &PhpType) -> Option<Vec<Vec<(PhpType, bool)>>> {
    ty.union_members()
        .into_iter()
        .map(|member| {
            explicit_key_shape_entries(member)?
                .into_iter()
                .map(|entry| {
                    let literal = crate::php_type::shape_key_literal(entry.key.as_deref()?)?;
                    Some((literal, entry.optional))
                })
                .collect()
        })
        .collect()
}

/// The union of `members`, or `None` when there are none or too many.
fn literal_key_union(members: impl IntoIterator<Item = PhpType>) -> Option<PhpType> {
    let mut distinct: Vec<PhpType> = Vec::new();
    for member in members {
        if !distinct.contains(&member) {
            if distinct.len() == MAX_KEY_ALTERNATIVES {
                return None;
            }
            distinct.push(member);
        }
    }
    match distinct.len() {
        0 => None,
        1 => distinct.pop(),
        _ => Some(PhpType::union(distinct)),
    }
}

/// `array_keys()` of an array shape, keeping the literal keys.
///
/// Without a search value every key comes back in order, so a shape whose
/// entries are all present gives a shape of its keys. A search value keeps
/// only some of them, and an optional entry leaves the position of every
/// later key open, so either gives a list of the possible keys instead.
fn shape_keys_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let alternatives = shape_key_alternatives(&args.arg_raw_type(0)?)?;
    let searched = args.has_arg(1);
    let mut results: Vec<PhpType> = Vec::new();
    for keys in alternatives {
        let result = if keys.is_empty() {
            PhpType::array_shape(Vec::new())
        } else if !searched && keys.iter().all(|(_, optional)| !optional) {
            PhpType::array_shape(
                keys.into_iter()
                    .map(|(key, _)| ShapeEntry {
                        key: None,
                        value_type: key,
                        optional: false,
                    })
                    .collect(),
            )
        } else {
            PhpType::list(literal_key_union(keys.into_iter().map(|(key, _)| key))?)
        };
        if !results.contains(&result) {
            results.push(result);
        }
    }
    match results.len() {
        1 => results.pop(),
        _ => Some(PhpType::union(results)),
    }
}

/// `array_key_first()` (`first`) or `array_key_last()` of an array shape.
///
/// The answer is the key of the first entry from that end that is certainly
/// present, or of any optional entry before it. A shape whose entries may
/// all be missing may be empty, which answers `null`.
fn shape_end_key_type(raw: &PhpType, first: bool) -> Option<PhpType> {
    let mut keys: Vec<PhpType> = Vec::new();
    for mut alternative in shape_key_alternatives(raw)? {
        if !first {
            alternative.reverse();
        }
        let mut present = false;
        for (key, optional) in alternative {
            keys.push(key);
            if !optional {
                present = true;
                break;
            }
        }
        if !present {
            keys.push(PhpType::null());
        }
    }
    literal_key_union(keys)
}

/// `array_column($array, $column_key, $index_key)` over rows that are
/// arrays or objects. Mirrors PHPStan's `ArrayColumnHelper`.
///
/// A constant shape of rows gives the shape `array_column()` builds: each
/// row's column in order, a row lacking it skipped, keyed by the row's index
/// column when that holds a literal key (a later row overwriting an earlier
/// one of the same key) and appended otherwise. Any other array gives a list
/// of the column's type, or an array keyed by the index column's.
///
/// `None` when a key argument is not a literal (or `null`) or a row is
/// neither an array nor an object of a class that can be loaded, which
/// leaves the stub's `array`.
fn array_column_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if (0..3).any(|i| args.is_spread(i) || args.is_named(i)) {
        return None;
    }
    let column = array_column_key_arg(args, 1)?;
    let index = if args.has_arg(2) {
        array_column_key_arg(args, 2)?
    } else {
        None
    };
    let input = args.arg_raw_type(0)?;
    if let TypeKind::ArrayShape(rows) = input.kind()
        && let Some(shape) = array_column_shape(args, rows, column.as_deref(), index.as_deref())
    {
        return Some(shape);
    }
    if input.is_empty_array_shape() {
        return Some(PhpType::array_shape(Vec::new()));
    }
    let row = input.iterable_element_type()?;
    let (value, certain) = match row_offset(args, &row, column.as_deref())? {
        RowOffset::Never => return Some(PhpType::array_shape(Vec::new())),
        RowOffset::Certain(value) => (value, true),
        RowOffset::Maybe(value) => (value, false),
    };
    let non_empty = certain && input.is_provably_non_empty();
    let Some(index) = index else {
        let list = PhpType::list(value);
        return Some(if non_empty {
            list.non_empty_array_form()
        } else {
            list
        });
    };
    let normalize = super::array_shape_writes::normalize_array_key_type_keeping_literals;
    let key = match row_offset(args, &row, Some(&index))? {
        RowOffset::Never => PhpType::int(),
        RowOffset::Certain(key) => normalize(&key)?,
        // A row without the index column is appended, under an int key
        // that absorbs every literal int one.
        // A key nothing is known about is cast to some key.
        RowOffset::Maybe(key) if key.is_mixed() => PhpType::named(crate::atom::atom("array-key")),
        RowOffset::Maybe(key) => {
            // An array is no key: PHP 8 throws for one, so only the rows
            // holding something else leave a key behind, beside the int a
            // row without the column is appended under.
            let kept: Vec<PhpType> = key
                .union_members()
                .into_iter()
                .filter(|member| !member.is_array_like())
                .cloned()
                .collect();
            let key = if kept.is_empty() {
                PhpType::int()
            } else {
                normalize(&PhpType::union(kept))?
            };
            let mut members: Vec<PhpType> = key
                .union_members()
                .into_iter()
                .filter(|member| !member.is_int_literal() && !member.is_int())
                .cloned()
                .collect();
            members.push(PhpType::int());
            PhpType::union(members)
        }
    };
    let array = PhpType::generic_array(key, value);
    Some(if non_empty {
        array.non_empty_array_form()
    } else {
        array
    })
}

/// A column or index key argument of `array_column()`: `Some(Some(keys))`
/// for a literal key or a union of them, `Some(None)` for `null`, and
/// `None` for anything that cannot be read.
fn array_column_key_arg(args: &dyn ArrayFuncArgs, index: usize) -> Option<Option<Vec<String>>> {
    let ty = args.arg_raw_type(index)?;
    if ty.is_null() {
        return Some(None);
    }
    ty.union_members()
        .into_iter()
        .map(literal_array_key)
        .collect::<Option<_>>()
        .map(Some)
}

/// The shape key a literal int or string is stored under, with a decimal
/// integer string cast to the integer the way PHP casts an array key.
fn literal_array_key(ty: &PhpType) -> Option<String> {
    match ty.as_literal()? {
        literal @ LiteralValue::Int(_) => Some(literal.parse_i64()?.to_string()),
        literal @ LiteralValue::String(_) => {
            let content = literal.string_content()?;
            if let Some(n) = canonical_int_key(&content) {
                return Some(n.to_string());
            }
            // A key spelled like a class constant would be read back as one.
            (!content.contains("::")).then(|| content.into_owned())
        }
        LiteralValue::Float(_) => None,
    }
}

/// What `array_column()` reads out of one row at a key.
enum RowOffset {
    /// No row has the key, so every row is skipped.
    Never,
    /// Every row has the key, holding this.
    Certain(PhpType),
    /// A row may lack the key; the ones that have it hold this.
    Maybe(PhpType),
}

/// The value a row of type `row` holds at any of `keys` (the whole row for
/// `None`), or `None` when the row is neither an array nor an object
/// [`ArrayFuncArgs::object_property`] can read.
fn row_offset(
    args: &dyn ArrayFuncArgs,
    row: &PhpType,
    keys: Option<&[String]>,
) -> Option<RowOffset> {
    let Some(keys) = keys else {
        return Some(RowOffset::Certain(row.clone()));
    };
    let mut values: Vec<PhpType> = Vec::new();
    let mut certain = true;
    for member in row.union_members() {
        for key in keys {
            match member.kind() {
                TypeKind::ArrayShape(_) => match member.shape_entry(key) {
                    Some(entry) => {
                        certain &= !entry.optional;
                        values.push(entry.value_type.clone());
                    }
                    None => certain = false,
                },
                _ if member.is_array_like() && !member.is_iterable() => {
                    certain = false;
                    values.push(
                        member
                            .iterable_element_type()
                            .unwrap_or_else(PhpType::mixed),
                    );
                }
                _ if member.is_object_like() => match args.object_property(member, key)? {
                    ObjectProperty::Readable(value) => values.push(value),
                    ObjectProperty::Missing => {}
                    // One row that may or may not have the property says
                    // nothing certain about any of them.
                    ObjectProperty::MaybeReadable(value) => return Some(RowOffset::Maybe(value)),
                    ObjectProperty::Unknown => return Some(RowOffset::Maybe(PhpType::mixed())),
                },
                _ => return None,
            }
        }
    }
    Some(match values.len() {
        0 => RowOffset::Never,
        _ if certain => RowOffset::Certain(PhpType::union(values)),
        _ => RowOffset::Maybe(PhpType::union(values)),
    })
}

/// [`array_column_type`] over a constant shape of `rows`, or `None` when a
/// row may or may not have the column, or its key cannot be told.
fn array_column_shape(
    args: &dyn ArrayFuncArgs,
    rows: &[ShapeEntry],
    column: Option<&[String]>,
    index: Option<&[String]>,
) -> Option<PhpType> {
    let mut entries: Vec<ShapeEntry> = Vec::new();
    let mut next_index: i64 = 0;
    // An optional entry appended leaves the position of every later one open.
    let mut appended_optional = false;
    for row in rows {
        let value = match row_offset(args, &row.value_type, column)? {
            RowOffset::Never => continue,
            RowOffset::Certain(value) => value,
            RowOffset::Maybe(_) => return None,
        };
        let key = match index {
            None => None,
            Some(index) => match row_offset(args, &row.value_type, Some(index))? {
                RowOffset::Never => None,
                RowOffset::Certain(key) => Some(literal_array_key(&key)?),
                RowOffset::Maybe(_) => return None,
            },
        };
        let key = match key {
            Some(key) => {
                if let Some(n) = canonical_int_key(&key) {
                    next_index = next_index.max(n + 1);
                }
                key
            }
            None => {
                if appended_optional {
                    return None;
                }
                appended_optional = row.optional;
                let key = next_index.to_string();
                next_index += 1;
                key
            }
        };
        match entries
            .iter_mut()
            .find(|entry| entry.key.as_deref() == Some(key.as_str()))
        {
            Some(entry) if !entry.optional && !row.optional => entry.value_type = value,
            Some(_) => return None,
            None => entries.push(ShapeEntry {
                key: Some(key),
                value_type: value,
                optional: row.optional,
            }),
        }
    }
    Some(shape_with_implicit_keys(entries))
}

/// `array_slice($array, $offset, $length, $preserve_keys)`.
///
/// A constant shape with a literal offset and length is sliced entry by
/// entry, following PHPStan's `ConstantArrayType::sliceArray()`: an optional
/// entry before the slice may push a later one into it, so entries near the
/// edges become optional. Anything else keeps the array's key and value
/// types, its int keys renumbered unless `$preserve_keys` is `true`, and
/// may come back empty.
fn array_slice_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if (0..4).any(|i| args.is_spread(i) || args.is_named(i)) {
        return None;
    }
    let input = args.arg_raw_type(0)?;
    let literal = |index: usize| {
        args.arg_raw_type(index)
            .and_then(|t| t.as_literal().and_then(LiteralValue::parse_i64))
    };
    let offset = literal(1);
    let length = if args.has_arg(2) {
        match args.arg_raw_type(2) {
            Some(t) if t.is_null() => Some(None),
            _ => literal(2).map(Some),
        }
    } else {
        Some(None)
    };
    let preserve = args.has_arg(3) && args.bool_literal(3) == Some(true);
    // A length that is at least one, whatever it is.
    let length_positive = args.has_arg(2)
        && args
            .arg_raw_type(2)
            .and_then(|t| crate::type_engine::type_ops::int_interval::IntInterval::of_int(&t))
            .is_some_and(|range| range.min.is_some_and(|min| min >= 1));
    if input.is_mixed() {
        return Some(PhpType::generic_array(PhpType::mixed(), PhpType::mixed()));
    }
    if length == Some(Some(0)) {
        return Some(PhpType::array_shape(Vec::new()));
    }
    if let (Some(offset), Some(length)) = (offset, length)
        && input
            .union_members()
            .iter()
            .all(|member| matches!(member.kind(), TypeKind::ArrayShape(_)))
    {
        let mut shapes: Vec<PhpType> = Vec::new();
        let mut sliced_all = true;
        for member in input.union_members() {
            match explicit_key_shape_entries(member)
                .and_then(|entries| slice_entries(entries, offset, length, preserve))
            {
                Some(shape) if !shapes.contains(&shape) => shapes.push(shape),
                Some(_) => {}
                None => sliced_all = false,
            }
        }
        if sliced_all {
            return Some(PhpType::union(shapes));
        }
    }
    // A shape sliced where it cannot be told keeps its string keys, and its
    // int keys too when they are preserved.
    if let TypeKind::ArrayShape(entries) = input.kind() {
        let mut keys: Vec<PhpType> = Vec::new();
        for entry in explicit_key_shape_entries(&input)? {
            let key = crate::php_type::shape_key_literal(entry.key.as_deref()?)?;
            keys.push(if !preserve && key.is_int_literal() {
                PhpType::int()
            } else {
                key
            });
        }
        let values = PhpType::join_runtime_value_types(
            entries.iter().map(|e| e.value_type.clone()).collect(),
        );
        return Some(match literal_key_union(keys) {
            Some(key) => {
                PhpType::generic_array(PhpType::join_runtime_value_types(vec![key]), values)
            }
            None => PhpType::array_shape(Vec::new()),
        });
    }
    let mut results: Vec<PhpType> = Vec::new();
    for member in input.generalized_array().union_members() {
        let (form, key, value) = generic_array_parts(member).or_else(|| {
            let value = member.iterable_element_type()?;
            Some((ArrayForm::Array, array_key_domain(member), value))
        })?;
        // A slice from the start keeps the first entry when it takes any.
        let keeps_first = form.is_non_empty()
            && offset == Some(0)
            && (length_positive || length.is_some_and(|length| length.is_none_or(|n| n > 0)));
        // A list keeps its keys in sequence only from its start.
        let stays_list = form.is_list() && (!preserve || offset == Some(0));
        let result = if value.is_mixed() && key.is_none() && (stays_list || !form.is_list()) {
            let name = if stays_list {
                form.name(keeps_first)
            } else {
                ArrayForm::Array.name(keeps_first)
            };
            PhpType::named(crate::atom::atom(name))
        } else if stays_list {
            PhpType::generic(form.name(keeps_first), vec![value])
        } else if form.is_list() {
            PhpType::generic_array(PhpType::int_range("0", "max"), value)
        } else {
            let name = ArrayForm::Array.name(keeps_first);
            match key {
                // Renumbered int keys make a list.
                Some(key) if !preserve && key.is_subtype_of(&PhpType::int()) => {
                    PhpType::generic(ArrayForm::List.name(keeps_first), vec![value])
                }
                Some(key) => PhpType::generic(name, vec![key, value]),
                None => PhpType::generic(name, vec![value]),
            }
        };
        if !results.contains(&result) {
            results.push(result);
        }
    }
    Some(PhpType::union(results))
}

/// `array_reverse($array, $preserve_keys)`: the entries in reverse order,
/// int keys renumbered unless `$preserve_keys` is `true`.
fn array_reverse_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if (0..2).any(|i| args.is_spread(i) || args.is_named(i)) {
        return None;
    }
    let input = args.arg_raw_type(0)?;
    let preserve = args.bool_literal(1) == Some(true);
    if input.is_mixed() {
        return Some(PhpType::generic_array(PhpType::mixed(), PhpType::mixed()));
    }
    let members = input.union_members();
    if members
        .iter()
        .all(|member| matches!(member.kind(), TypeKind::ArrayShape(_)))
        && let Some(shapes) = members
            .iter()
            .map(|member| {
                let mut entries = explicit_key_shape_entries(member)?;
                entries.reverse();
                renumber_slice(entries, preserve)
            })
            .collect::<Option<Vec<_>>>()
    {
        let mut distinct: Vec<PhpType> = Vec::new();
        for shape in shapes {
            if !distinct.contains(&shape) {
                distinct.push(shape);
            }
        }
        return Some(PhpType::union(distinct));
    }
    // Renumbered int keys always make a list, even where optional entries
    // leave which value lands where open.
    if !preserve
        && members.iter().all(|member| {
            explicit_key_shape_entries(member).is_some_and(|entries| {
                entries
                    .iter()
                    .all(|e| e.key.as_deref().and_then(canonical_int_key).is_some())
            })
        })
    {
        let value = input.iterable_element_type()?;
        let list = PhpType::list(value);
        return Some(if input.is_provably_non_empty() {
            list.non_empty_array_form()
        } else {
            list
        });
    }
    let container = input.generalized_array();
    // A reversed list keeps its keys only with them counting down.
    if preserve
        && let Some((form, _, value)) = generic_array_parts(&container)
        && form.is_list()
    {
        let array = PhpType::generic_array(PhpType::int_range("0", "max"), value);
        return Some(if form.is_non_empty() {
            array.non_empty_array_form()
        } else {
            array
        });
    }
    Some(container)
}

/// `array_chunk($array, $length, $preserve_keys)`: the array in pieces of
/// `$length` entries, each piece non-empty, renumbered from zero unless
/// `$preserve_keys` asks for the original keys.
///
/// A constant shape with a literal length is cut piece by piece; a piece
/// that may be empty, because every entry in it is optional, is an optional
/// entry of the result.
fn array_chunk_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if (0..3).any(|i| args.is_spread(i) || args.is_named(i)) {
        return None;
    }
    let input = args.arg_raw_type(0)?;
    let preserve = args.bool_literal(2) == Some(true);
    // Each length a literal (or a union of a few literals) may be.
    let sizes: Option<Vec<i64>> = args.arg_raw_type(1).and_then(|t| {
        let members = t.union_members();
        (members.len() <= 4)
            .then(|| {
                members
                    .into_iter()
                    .map(|m| {
                        m.as_literal()
                            .and_then(LiteralValue::parse_i64)
                            .filter(|&n| n >= 1)
                    })
                    .collect()
            })
            .flatten()
    });
    let members = input.union_members();
    if let Some(sizes) = sizes
        && members
            .iter()
            .all(|member| matches!(member.kind(), TypeKind::ArrayShape(_)))
        && let Some(shapes) = members
            .iter()
            .flat_map(|member| {
                sizes
                    .iter()
                    .map(|&size| chunk_shape(member, size, preserve))
            })
            .collect::<Option<Vec<_>>>()
    {
        let mut distinct: Vec<PhpType> = Vec::new();
        for shape in shapes {
            if !distinct.contains(&shape) {
                distinct.push(shape);
            }
        }
        return Some(PhpType::union(distinct));
    }
    let bare = matches!(input.kind(), TypeKind::Named(_));
    let container = input.generalized_array();
    let (form, key, value) = generic_array_parts(&container).or_else(|| {
        // A union of arrays chunks into pieces of any of their entries.
        let value = container.iterable_element_type()?;
        let key = array_key_domain(&container);
        Some((ArrayForm::Array, key, value))
    })?;
    let chunk = if bare {
        PhpType::named(crate::atom::atom(if preserve {
            "non-empty-array"
        } else {
            "non-empty-list"
        }))
    } else if preserve && !form.is_list() {
        PhpType::generic("non-empty-array", key.into_iter().chain([value]).collect())
    } else {
        PhpType::generic("non-empty-list", vec![value])
    };
    let outer = PhpType::list(chunk);
    Some(if input.is_provably_non_empty() {
        outer.non_empty_array_form()
    } else {
        outer
    })
}

fn chunk_shape(input: &PhpType, size: i64, preserve: bool) -> Option<PhpType> {
    let entries = explicit_key_shape_entries(input)?;
    let count = entries.len() as i64;
    let mut chunks: Vec<ShapeEntry> = Vec::new();
    let mut start = 0;
    while start < count {
        let chunk = slice_entries(entries.clone(), start, Some(size), true)?;
        let mut chunk_entries = explicit_key_shape_entries(&chunk)?;
        // Without the keys, every key is renumbered, which an optional
        // entry before another leaves uncertain.
        let chunk = if preserve {
            chunk
        } else {
            if chunk_entries
                .iter()
                .rev()
                .skip(1)
                .any(|entry| entry.optional)
            {
                return None;
            }
            for (i, entry) in chunk_entries.iter_mut().enumerate() {
                entry.key = Some(i.to_string());
            }
            shape_with_implicit_keys(chunk_entries.clone())
        };
        let optional = chunk_entries.iter().all(|entry| entry.optional);
        // A piece after one that may be missing has no fixed position.
        if chunks.last().is_some_and(|previous| previous.optional) && !optional {
            return None;
        }
        chunks.push(ShapeEntry {
            key: Some(chunks.len().to_string()),
            value_type: chunk,
            optional,
        });
        start += size;
    }
    Some(shape_with_implicit_keys(chunks))
}

/// [`array_slice_type`] over one shape's entries, keys spelled out. `None`
/// when renumbering after an optional entry leaves a key uncertain.
fn slice_entries(
    mut entries: Vec<ShapeEntry>,
    mut offset: i64,
    length: Option<i64>,
    preserve: bool,
) -> Option<PhpType> {
    let count = entries.len() as i64;
    if count == 0 {
        return Some(PhpType::array_shape(Vec::new()));
    }
    let mut length = length.unwrap_or(count);
    // A negative offset or length cannot reach past the array's start.
    if count + offset <= 0 {
        offset = 0;
    }
    if count + length <= 0 {
        length = 0;
    }
    if length == 0 || (offset < 0 && length < 0 && offset - length >= 0) {
        return Some(PhpType::array_shape(Vec::new()));
    }
    if length < 0 {
        // Drop the last entries; each optional one dropped may have been
        // standing in for an earlier entry, which becomes optional.
        let new_len = (count + length) as usize;
        let mut removed_optional = entries[new_len..].iter().filter(|e| e.optional).count();
        entries.truncate(new_len);
        for entry in entries.iter_mut().rev() {
            if removed_optional == 0 {
                break;
            }
            if !entry.optional {
                entry.optional = true;
                removed_optional -= 1;
            }
        }
        return slice_entries(entries, offset, None, preserve);
    }
    if offset < 0 {
        // Slice the reversed entries from the left, then turn them back.
        let back = -offset;
        let reversed_length = length.min(back);
        entries.reverse();
        let sliced = slice_entries(entries, back - reversed_length, Some(reversed_length), true)?;
        let mut sliced = explicit_key_shape_entries(&sliced)?;
        sliced.reverse();
        return renumber_slice(sliced, preserve);
    }
    if offset > 0 {
        let skip = (offset as usize).min(entries.len());
        let mut ignored_optional = entries[..skip].iter().filter(|e| e.optional).count();
        let mut rest: Vec<ShapeEntry> = entries.split_off(skip);
        for entry in &mut rest {
            if ignored_optional == 0 {
                break;
            }
            if !entry.optional {
                entry.optional = true;
                ignored_optional -= 1;
            }
        }
        return slice_entries(rest, 0, Some(length), preserve);
    }
    let count = entries.len() as i64;
    let mut sliced: Vec<ShapeEntry> = Vec::new();
    let mut required = 0;
    let mut has_optional = false;
    for (i, mut entry) in entries.into_iter().enumerate() {
        if required >= length {
            break;
        }
        if entry.optional {
            has_optional = true;
        } else {
            required += 1;
        }
        let last = required >= length || i as i64 + 1 >= count;
        if last && length < count && has_optional {
            entry.optional = true;
        }
        sliced.push(entry);
    }
    renumber_slice(sliced, preserve)
}

/// A sliced shape with its int keys renumbered from zero, unless kept.
/// `None` when an optional int-keyed entry leaves a later key uncertain.
fn renumber_slice(entries: Vec<ShapeEntry>, preserve: bool) -> Option<PhpType> {
    if preserve {
        return Some(shape_with_implicit_keys(entries));
    }
    let mut next = 0;
    let mut renumbered = Vec::with_capacity(entries.len());
    let mut uncertain = false;
    for mut entry in entries {
        if entry.key.as_deref().and_then(canonical_int_key).is_some() {
            if uncertain {
                return None;
            }
            entry.key = Some(next.to_string());
            next += 1;
            uncertain |= entry.optional;
        }
        renumbered.push(entry);
    }
    Some(shape_with_implicit_keys(renumbered))
}

/// How many pieces a literal `str_split()` folds into a shape.
const MAX_SPLIT_PIECES: usize = 64;

/// `str_split()` and (`multibyte`) `mb_str_split()`: the string in chunks
/// of `$length` bytes or characters. Since PHP 8.2 the empty string gives
/// `[]`, so every chunk is non-empty.
fn str_split_type(args: &dyn ArrayFuncArgs, multibyte: bool) -> Option<PhpType> {
    let length = if args.has_arg(1) {
        args.arg_raw_type(1)
            .and_then(|t| t.as_literal().and_then(LiteralValue::parse_i64))
    } else {
        Some(1)
    };
    // Another encoding counts characters differently.
    let utf8 = !args.has_arg(2)
        || args
            .arg_raw_type(2)
            .and_then(|t| t.as_literal()?.string_content().map(|c| c.into_owned()))
            .is_some_and(|encoding| {
                encoding.eq_ignore_ascii_case("UTF-8") || encoding.eq_ignore_ascii_case("UTF8")
            });
    let subject = args.arg_raw_type(0)?;
    if let Some(length) = length.filter(|&n| n >= 1)
        && (!multibyte || utf8)
        && let Some(texts) = subject
            .union_members()
            .into_iter()
            .map(|member| Some(member.as_literal()?.string_content()?.into_owned()))
            .collect::<Option<Vec<String>>>()
    {
        let mut shapes: Vec<PhpType> = Vec::new();
        for text in &texts {
            let pieces: Vec<String> = if multibyte {
                let chars: Vec<char> = text.chars().collect();
                chars
                    .chunks(length as usize)
                    .map(|chunk| chunk.iter().collect())
                    .collect()
            } else {
                text.as_bytes()
                    .chunks(length as usize)
                    .map(|chunk| std::str::from_utf8(chunk).ok().map(str::to_string))
                    .collect::<Option<_>>()?
            };
            if pieces.len() > MAX_SPLIT_PIECES {
                return None;
            }
            let shape = PhpType::array_shape(
                pieces
                    .iter()
                    .map(|piece| ShapeEntry {
                        key: None,
                        value_type: PhpType::literal_string_value(piece),
                        optional: false,
                    })
                    .collect(),
            );
            if !shapes.contains(&shape) {
                shapes.push(shape);
            }
        }
        return Some(PhpType::union(shapes));
    }
    let list = PhpType::list(PhpType::non_empty_string());
    Some(if subject.is_subtype_of(&PhpType::non_empty_string()) {
        list.non_empty_array_form()
    } else {
        list
    })
}

/// `array_keys($array)` (`keys`) or `array_values($array)` of a generic
/// array: a list of its keys or values, non-empty when the array is. A
/// list's keys count up from zero.
fn keys_or_values_list(args: &dyn ArrayFuncArgs, keys: bool) -> Option<PhpType> {
    if args.is_spread(0) || args.is_named(0) {
        return None;
    }
    let input = args.arg_raw_type(0)?;
    let (form, key, value) = generic_array_parts(&input)?;
    let element = if keys {
        if form.is_list() {
            PhpType::int_range("0", "max")
        } else {
            match key {
                // `array-key` already names what a key can be.
                Some(key) if key.is_array_key() => key,
                Some(key) => {
                    super::array_shape_writes::normalize_array_key_type_keeping_literals(&key)?
                }
                None => return None,
            }
        }
    } else {
        value
    };
    // A bare `list` stays bare; PHPStan spells the values of a bare `array`
    // out as `list<mixed>`.
    let list = if element.is_mixed() && form.is_list() && matches!(input.kind(), TypeKind::Named(_))
    {
        PhpType::named(crate::atom::atom("list"))
    } else {
        PhpType::list(element)
    };
    Some(if form.is_non_empty() {
        list.non_empty_array_form()
    } else {
        list
    })
}

/// The generic array forms a key-preserving function reads apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArrayForm {
    Array,
    NonEmptyArray,
    List,
    NonEmptyList,
}

impl ArrayForm {
    fn is_list(self) -> bool {
        matches!(self, ArrayForm::List | ArrayForm::NonEmptyList)
    }

    fn is_non_empty(self) -> bool {
        matches!(self, ArrayForm::NonEmptyArray | ArrayForm::NonEmptyList)
    }

    fn name(self, non_empty: bool) -> &'static str {
        match (self.is_list(), non_empty) {
            (true, true) => "non-empty-list",
            (true, false) => "list",
            (false, true) => "non-empty-array",
            (false, false) => "array",
        }
    }
}

/// A generic array as its form, its written key type (`None` for the
/// implicit one of `array<V>` and `V[]`) and its value type.
fn generic_array_parts(ty: &PhpType) -> Option<(ArrayForm, Option<PhpType>, PhpType)> {
    match ty.kind() {
        TypeKind::Generic(generic) => {
            let form = match generic.name.to_ascii_lowercase().as_str() {
                "array" => ArrayForm::Array,
                "non-empty-array" => ArrayForm::NonEmptyArray,
                "list" => ArrayForm::List,
                "non-empty-list" => ArrayForm::NonEmptyList,
                _ => return None,
            };
            match generic.args.as_slice() {
                [value] => Some((form, None, value.clone())),
                [key, value] if !form.is_list() => Some((form, Some(key.clone()), value.clone())),
                _ => None,
            }
        }
        TypeKind::Array(element) => Some((ArrayForm::Array, None, element.clone())),
        TypeKind::Named(name) => {
            let form = match name.to_ascii_lowercase().as_str() {
                "array" => ArrayForm::Array,
                "non-empty-array" => ArrayForm::NonEmptyArray,
                "list" => ArrayForm::List,
                "non-empty-list" => ArrayForm::NonEmptyList,
                _ => return None,
            };
            Some((form, None, PhpType::mixed()))
        }
        _ => None,
    }
}

/// `array_count_values()`: how often each int and string value occurs,
/// keyed by the value. PHP skips (with a warning) every other value, so an
/// array of only those counts nothing.
fn array_count_values_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let input = args.arg_raw_type(0)?;
    let (values, non_empty) = match input.kind() {
        TypeKind::ArrayShape(entries) => {
            let counted = |value: &PhpType| value.is_int_subtype() || value.is_string_subtype();
            let non_empty = entries
                .iter()
                .any(|entry| !entry.optional && counted(&entry.value_type));
            let values: Vec<PhpType> = entries.iter().map(|e| e.value_type.clone()).collect();
            (PhpType::union(values), non_empty)
        }
        _ => {
            let (form, _, value) = generic_array_parts(&input)?;
            (value, form.is_non_empty())
        }
    };
    let mut kept: Vec<PhpType> = Vec::new();
    let mut dropped = false;
    for member in values.union_members() {
        if member.is_int_subtype() || member.is_string_subtype() {
            kept.push(member.clone());
        } else if member.is_mixed() || member.is_array_key() || member.is_named_ci("scalar") {
            return None;
        } else {
            dropped = true;
        }
    }
    if kept.is_empty() {
        return Some(PhpType::array_shape(Vec::new()));
    }
    let key = super::array_shape_writes::normalize_array_key_type_keeping_literals(
        &PhpType::union(kept),
    )?;
    let non_empty = non_empty && (!dropped || matches!(input.kind(), TypeKind::ArrayShape(_)));
    let name = if non_empty {
        "non-empty-array"
    } else {
        "array"
    };
    Some(PhpType::generic(
        name,
        vec![key, PhpType::int_range("1", "max")],
    ))
}

/// `CASE_LOWER` and `CASE_UPPER`.
const CASE_LOWER: i64 = 0;
const CASE_UPPER: i64 = 1;

/// `array_change_key_case()`: the same entries with every string key
/// lower- or upper-cased.
///
/// A mode that may be either gives the keys of both. `None` when two keys
/// of a shape would collide, or the input is not an array this reads.
fn array_change_key_case_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let modes: Vec<i64> = if args.has_arg(1) {
        match args.arg_atom_text(1).as_deref() {
            Some("CASE_LOWER") => vec![CASE_LOWER],
            Some("CASE_UPPER") => vec![CASE_UPPER],
            _ => match args
                .arg_raw_type(1)
                .as_ref()
                .and_then(|t| t.as_literal()?.parse_i64())
            {
                Some(mode @ (CASE_LOWER | CASE_UPPER)) => vec![mode],
                _ => vec![CASE_LOWER, CASE_UPPER],
            },
        }
    } else {
        vec![CASE_LOWER]
    };
    let input = args.arg_raw_type(0)?;
    if let TypeKind::ArrayShape(_) = input.kind() {
        let entries = explicit_key_shape_entries(&input)?;
        if let [mode] = modes.as_slice() {
            let mut changed: Vec<ShapeEntry> = Vec::with_capacity(entries.len());
            for entry in entries {
                let key = entry.key.as_deref()?;
                let key = if canonical_int_key(key).is_some() {
                    key.to_string()
                } else {
                    change_case(key, *mode)
                };
                if changed
                    .iter()
                    .any(|e| e.key.as_deref() == Some(key.as_str()))
                {
                    return None;
                }
                changed.push(ShapeEntry {
                    key: Some(key),
                    ..entry
                });
            }
            return Some(shape_with_implicit_keys(changed));
        }
        let non_empty = entries.iter().any(|entry| !entry.optional);
        let mut keys: Vec<PhpType> = Vec::new();
        for entry in &entries {
            let key = crate::php_type::shape_key_literal(entry.key.as_deref()?)?;
            for &mode in &modes {
                let changed = change_key_type_case(&key, mode)?;
                if !keys.contains(&changed) {
                    keys.push(changed);
                }
            }
        }
        let values =
            PhpType::join_runtime_value_types(entries.into_iter().map(|e| e.value_type).collect());
        let name = if non_empty {
            "non-empty-array"
        } else {
            "array"
        };
        return Some(PhpType::generic(
            name,
            vec![literal_key_union(keys)?, values],
        ));
    }
    let (form, key, value) = generic_array_parts(&input)?;
    let Some(key) = key else {
        return Some(input);
    };
    let mut keys: Vec<PhpType> = Vec::new();
    for &mode in &modes {
        let changed = if let [_] = modes.as_slice() {
            change_key_type_case(&key, mode)?
        } else {
            unknown_case_key_type(&key)?
        };
        if !keys.contains(&changed) {
            keys.push(changed);
        }
    }
    Some(PhpType::generic(
        form.name(form.is_non_empty()),
        vec![PhpType::union(keys), value],
    ))
}

fn change_case(key: &str, mode: i64) -> String {
    // PHP changes the case of ASCII letters only.
    if mode == CASE_UPPER {
        key.to_ascii_uppercase()
    } else {
        key.to_ascii_lowercase()
    }
}

/// A key type with its strings in the case `mode` asks for: a literal is
/// recased, and any other string keeps its emptiness but takes the case.
fn change_key_type_case(key: &PhpType, mode: i64) -> Option<PhpType> {
    let case = if mode == CASE_UPPER {
        "uppercase-string"
    } else {
        "lowercase-string"
    };
    let members: Vec<PhpType> = key
        .union_members()
        .into_iter()
        .map(|member| {
            if member.is_int_subtype() {
                return Some(member.clone());
            }
            if let Some(content) = member.as_literal().and_then(|l| l.string_content()) {
                return Some(PhpType::literal_string_value(change_case(&content, mode)));
            }
            if member.is_array_key() {
                return Some(PhpType::union(vec![
                    PhpType::int(),
                    PhpType::named(crate::atom::atom(case)),
                ]));
            }
            if !member.is_string_subtype() {
                return None;
            }
            let mut parts: Vec<PhpType> = string_emptiness(member).into_iter().collect();
            parts.push(PhpType::named(crate::atom::atom(case)));
            Some(if parts.len() == 1 {
                parts.pop()?
            } else {
                PhpType::intersection(parts)
            })
        })
        .collect::<Option<_>>()?;
    Some(PhpType::union(members))
}

/// A key type whose strings may come out in either case: a literal gives
/// both spellings, and any other string keeps only its emptiness.
fn unknown_case_key_type(key: &PhpType) -> Option<PhpType> {
    let mut members: Vec<PhpType> = Vec::new();
    for member in key.union_members() {
        if member.is_int_subtype() || member.is_array_key() {
            members.push(member.clone());
        } else if member.as_literal().is_some() {
            for mode in [CASE_LOWER, CASE_UPPER] {
                members.push(change_key_type_case(member, mode)?);
            }
        } else if member.is_string_subtype() {
            members.push(string_emptiness(member).unwrap_or_else(PhpType::string));
        } else {
            return None;
        }
    }
    Some(PhpType::union(members))
}

/// `non-falsy-string` or `non-empty-string` when a string type promises
/// it, which changing the case of its letters keeps.
fn string_emptiness(ty: &PhpType) -> Option<PhpType> {
    let names = ["non-falsy-string", "truthy-string", "non-empty-string"];
    let has = |name: &str| match ty.kind() {
        TypeKind::Named(n) => {
            n.eq_ignore_ascii_case(name)
                || (name == "non-empty-string"
                    && (n.eq_ignore_ascii_case("non-empty-lowercase-string")
                        || n.eq_ignore_ascii_case("non-empty-uppercase-string")))
        }
        TypeKind::Intersection(members) => members
            .iter()
            .any(|m| matches!(m.kind(), TypeKind::Named(n) if n.eq_ignore_ascii_case(name))),
        _ => false,
    };
    if has(names[0]) || has(names[1]) {
        Some(PhpType::named(crate::atom::atom("non-falsy-string")))
    } else if has(names[2]) {
        Some(PhpType::non_empty_string())
    } else {
        None
    }
}

/// `array_pad($array, $length, $value)`: the array with `$value` added as
/// needed, under new int keys. A length other than zero always leaves an
/// entry.
fn array_pad_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let input = args.arg_raw_type(0)?;
    let pad = args.arg_raw_type(2)?;
    let (form, key, value) = generic_array_parts(&input.generalized_array())?;
    let non_zero = args.arg_raw_type(1).is_some_and(|length| {
        length
            .union_members()
            .into_iter()
            .all(|member| member.is_subtype_of(&PhpType::parse("positive-int|negative-int")))
    });
    let value = PhpType::join_runtime_value_types(vec![value, pad]);
    let non_empty = form.is_non_empty() || non_zero;
    if form.is_list() {
        return Some(PhpType::generic(form.name(non_empty), vec![value]));
    }
    let key = key.unwrap_or_else(|| PhpType::union(vec![PhpType::int(), PhpType::string()]));
    let key = PhpType::join_runtime_value_types(vec![key, PhpType::int()]);
    Some(PhpType::generic(form.name(non_empty), vec![key, value]))
}

/// How many alternative pairs of shapes `array_combine()` folds before it
/// falls back to the arrays' element types.
const MAX_COMBINED_SHAPES: usize = 16;

/// `array_combine($keys, $values)`: each value under the key at the same
/// position.
///
/// Two constant arrays give a shape for every pair of their alternatives
/// whose lengths agree; PHP throws a `ValueError` for any other pair, so
/// none agreeing gives `never`. Anything else gives an array keyed by what
/// `$keys` holds.
fn array_combine_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let keys = args.arg_raw_type(0)?;
    let values = args.arg_raw_type(1)?;
    if keys.is_mixed() || values.is_mixed() {
        return Some(PhpType::array());
    }
    let shape_entries = |ty: &PhpType| -> Option<Vec<Vec<ShapeEntry>>> {
        ty.union_members()
            .into_iter()
            .map(|member| match member.kind() {
                TypeKind::ArrayShape(entries) if entries.iter().all(|e| !e.optional) => {
                    Some(entries.to_vec())
                }
                _ => None,
            })
            .collect()
    };
    if let (Some(key_shapes), Some(value_shapes)) = (shape_entries(&keys), shape_entries(&values))
        && key_shapes.len() * value_shapes.len() <= MAX_COMBINED_SHAPES
        && let Some(combined) = combine_shapes(&key_shapes, &value_shapes)
    {
        return Some(combined);
    }
    let key = fill_key_type(&keys.iterable_element_type()?)?;
    let value = values.iterable_element_type()?;
    let name = if keys.is_provably_non_empty() {
        "non-empty-array"
    } else {
        "array"
    };
    Some(PhpType::generic(name, vec![key, value]))
}

/// One shape per pair of key and value alternatives of the same length, or
/// `None` when a key cannot be read as one literal.
fn combine_shapes(
    key_shapes: &[Vec<ShapeEntry>],
    value_shapes: &[Vec<ShapeEntry>],
) -> Option<PhpType> {
    let mut results: Vec<PhpType> = Vec::new();
    for keys in key_shapes {
        for values in value_shapes
            .iter()
            .filter(|values| values.len() == keys.len())
        {
            let mut combined: Vec<ShapeEntry> = Vec::new();
            for (key, value) in keys.iter().zip(values) {
                let key = literal_array_key(&fill_key_type(&key.value_type)?)?;
                match combined.iter_mut().find(|e| e.key.as_deref() == Some(&key)) {
                    Some(existing) => existing.value_type = value.value_type.clone(),
                    None => combined.push(ShapeEntry {
                        key: Some(key),
                        value_type: value.value_type.clone(),
                        optional: false,
                    }),
                }
            }
            let shape = shape_with_implicit_keys(combined);
            if !results.contains(&shape) {
                results.push(shape);
            }
        }
    }
    Some(if results.is_empty() {
        PhpType::never()
    } else {
        PhpType::union(results)
    })
}

/// `array_fill_keys($keys, $value)`: `$value` under each of the keys.
///
/// A constant array of keys gives a shape, one per alternative of a union
/// of shapes, with a key optional when its entry is or when the entry may
/// hold one of several keys. `None` when a key is an object (named by its
/// `__toString()`), which leaves the stub's generic answer.
fn array_fill_keys_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let keys = args.arg_raw_type(0)?;
    let value = args.arg_raw_type(1)?;
    if keys
        .union_members()
        .iter()
        .all(|member| matches!(member.kind(), TypeKind::ArrayShape(_)))
    {
        let mut shapes: Vec<PhpType> = Vec::new();
        for member in keys.union_members() {
            let shape = fill_keys_shape(member, &value)?;
            if !shapes.contains(&shape) {
                shapes.push(shape);
            }
        }
        return Some(PhpType::union(shapes));
    }
    let (form, _, element) = generic_array_parts(&keys.generalized_array())?;
    let key = fill_key_type(&element)?;
    let name = if form.is_non_empty() {
        "non-empty-array"
    } else {
        "array"
    };
    Some(PhpType::generic(name, vec![key, value]))
}

fn fill_keys_shape(keys: &PhpType, value: &PhpType) -> Option<PhpType> {
    let TypeKind::ArrayShape(entries) = keys.kind() else {
        return None;
    };
    let mut filled: Vec<ShapeEntry> = Vec::new();
    for entry in entries.iter() {
        let members = entry.value_type.union_members();
        let optional = entry.optional || members.len() > 1;
        for member in members {
            let key = fill_key_type(member)?;
            let key = literal_array_key(&key)?;
            match filled.iter_mut().find(|e| e.key.as_deref() == Some(&key)) {
                Some(existing) => existing.optional &= optional,
                None => filled.push(ShapeEntry {
                    key: Some(key),
                    value_type: value.clone(),
                    optional,
                }),
            }
        }
    }
    Some(shape_with_implicit_keys(filled))
}

/// The key `array_fill_keys()` stores a value of type `ty` under: an int
/// as itself, anything else as the string it converts to, which PHP then
/// stores as an int when it is a decimal integer. `None` for an object or a
/// type whose string form cannot be told.
fn fill_key_type(ty: &PhpType) -> Option<PhpType> {
    let members: Vec<PhpType> = ty
        .union_members()
        .into_iter()
        .map(|member| {
            if member.is_int_subtype() {
                return Some(vec![member.clone()]);
            }
            if member.is_true() {
                return Some(vec![PhpType::literal_int("1")]);
            }
            if member.is_false() || member.is_null() {
                return Some(vec![PhpType::literal_string_value("")]);
            }
            if member.is_bool() {
                return Some(vec![
                    PhpType::literal_string_value(""),
                    PhpType::literal_int("1"),
                ]);
            }
            if let Some(value) = member.as_literal().and_then(|l| l.parse_f64()) {
                // Small values print the same in Rust as in PHP; larger
                // ones switch to PHP's exponent form.
                if !value.is_finite() || value.abs() >= 1e15 {
                    return None;
                }
                let text = format!("{value}");
                return Some(vec![match canonical_int_key(&text) {
                    Some(n) => PhpType::literal_int(n.to_string()),
                    None => PhpType::literal_string_value(text),
                }]);
            }
            if member.is_float_subtype() {
                return Some(vec![PhpType::intersection(vec![
                    PhpType::named(crate::atom::atom("numeric-string")),
                    PhpType::named(crate::atom::atom("uppercase-string")),
                ])]);
            }
            if member.is_string_subtype() {
                return super::array_shape_writes::normalize_array_key_type_keeping_literals(
                    member,
                )
                .map(|key| vec![key]);
            }
            None
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    Some(PhpType::join_runtime_value_types(members))
}

/// An array shape over `entries`, written positionally when they are the
/// required `0, 1, 2, …` a positional shape stands for.
fn shape_with_implicit_keys(mut entries: Vec<ShapeEntry>) -> PhpType {
    let sequential = entries.iter().enumerate().all(|(i, entry)| {
        !entry.optional && entry.key.as_deref().and_then(canonical_int_key) == Some(i as i64)
    });
    if sequential {
        for entry in &mut entries {
            entry.key = None;
        }
    }
    PhpType::array_shape(entries)
}

/// `array_filter` over an array shape, decided entry by entry.
///
/// Each entry's value is split into what the filter keeps and whether it
/// can drop it: an entry nothing survives is removed, one the filter may
/// drop becomes optional, and one it always keeps stays as it was. Without
/// a callback the split is truthiness; with one it is the type check a
/// callable string names, and a callback that may be any of several such
/// strings gives one shape per alternative. Any other callback says too
/// little about which entries survive, and the rule declines.
fn filter_shape_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let entries = explicit_key_shape_entries(&args.arg_raw_type(0)?)?;
    if !args.has_arg(1) {
        return filtered_shape(&entries, |value| {
            Some((value.truthy_type(), value.falsy_type().is_some()))
        });
    }
    // Only the default mode hands the type check the value.
    if args.has_arg(2) {
        return None;
    }
    let mut shapes: Vec<PhpType> = Vec::new();
    for guard in callback_guard_names(args, 1)? {
        let shape = filtered_shape(&entries, |value| args.guard_split(&guard, value))?;
        if !shapes.contains(&shape) {
            shapes.push(shape);
        }
    }
    match shapes.len() {
        0 => None,
        1 => shapes.pop(),
        _ => Some(PhpType::union(shapes)),
    }
}

/// Rebuild a filtered shape from `entries` (keys already explicit), given
/// how the filter splits one entry's value. `None` when it cannot say.
fn filtered_shape(
    entries: &[ShapeEntry],
    split: impl Fn(&PhpType) -> Option<(Option<PhpType>, bool)>,
) -> Option<PhpType> {
    let mut kept = Vec::with_capacity(entries.len());
    for entry in entries {
        let (accepted, may_reject) = split(&entry.value_type)?;
        let Some(value_type) = accepted else {
            continue;
        };
        kept.push(ShapeEntry {
            key: entry.key.clone(),
            value_type,
            optional: entry.optional || may_reject,
        });
    }
    Some(shape_with_implicit_keys(kept))
}

/// The type-check functions the callback at `index` names: one for
/// `'is_int'`, several for a variable holding one of a few such strings.
/// `None` when any alternative is something else.
fn callback_guard_names(args: &dyn ArrayFuncArgs, index: usize) -> Option<Vec<String>> {
    let callback = args.arg_raw_type(index)?;
    callback
        .union_members()
        .into_iter()
        .map(|member| {
            let content = member.as_literal()?.string_content()?;
            let name = content.trim_start_matches('\\');
            crate::type_engine::type_ops::condition_facts::type_guard_kind_from_name(name)?;
            Some(name.to_string())
        })
        .collect()
}

/// What the filter callback at index 1 proves about the value its
/// `param_index`th parameter is handed.
///
/// An inline function or callable string is read directly; a variable
/// holding one of several type-check names keeps whatever any of them
/// accepts.
fn filter_callback_narrowing(
    args: &dyn ArrayFuncArgs,
    param_index: usize,
    subject: &PhpType,
) -> Option<PhpType> {
    if let Some(narrowed) = args.callback_param_narrowing(1, param_index, subject) {
        return Some(narrowed);
    }
    // A named function receives the entry as its first argument only.
    if param_index != 0 {
        return None;
    }
    let mut accepted: Vec<PhpType> = Vec::new();
    for guard in callback_guard_names(args, 1)? {
        if let Some(kept) = args.guard_split(&guard, subject)?.0 {
            accepted.push(kept);
        }
    }
    match accepted.len() {
        0 => None,
        1 => accepted.pop(),
        _ => Some(PhpType::join_runtime_value_types(accepted)),
    }
}

/// The type of `max()`/`min()`'s result.
///
/// PHP gives the function two shapes: handed a single iterable it compares
/// the entries and returns one of them, and handed several values it
/// compares those. Either way the result is one of the values that went in,
/// so the answer is a member of what was passed rather than a new type.
///
/// Returns `None` as soon as one argument cannot be resolved, since a union
/// missing a member would be narrower than the call can promise.
fn min_max_type(args: &dyn ArrayFuncArgs, want_max: bool) -> Option<PhpType> {
    if !args.has_arg(1) {
        let iterable = args.arg_raw_type(0)?;
        // Every number a literal array holds is known, so the one that
        // comes out is too: for a union of such arrays, one per array.
        let shape_extreme = |member: &PhpType| match member.kind() {
            TypeKind::ArrayShape(entries) if entries.iter().all(|entry| !entry.optional) => {
                extreme_number(entries.iter().map(|entry| &entry.value_type), want_max)
            }
            _ => None,
        };
        let mut extremes: Vec<PhpType> = Vec::new();
        for member in iterable.union_members() {
            let Some(extreme) = shape_extreme(member) else {
                return iterable.iterable_element_type();
            };
            if !extremes.contains(&extreme) {
                extremes.push(extreme);
            }
        }
        return match extremes.len() {
            0 => iterable.iterable_element_type(),
            1 => extremes.pop(),
            _ => Some(PhpType::union(extremes)),
        };
    }

    let mut raw_args = Vec::new();
    let mut index = 0;
    while args.has_arg(index) {
        raw_args.push(args.arg_raw_type(index)?);
        index += 1;
    }
    if let Some(extreme) = extreme_number(raw_args.iter(), want_max) {
        return Some(extreme);
    }
    if let Some(extreme) = extreme_int_range(&raw_args, want_max) {
        return Some(extreme);
    }

    let mut members = Vec::new();
    for raw in &raw_args {
        let widened = widen_non_bool_scalar_literals(raw);
        for member in widened.union_members() {
            if !members.contains(member) {
                members.push(member.clone());
            }
        }
    }
    match members.len() {
        0 => None,
        1 => members.pop(),
        _ => Some(PhpType::union(members)),
    }
}

/// `array_fill($start, $count, $value)`: `$count` copies of `$value`, so
/// the entries are exactly the value's type, written-out value included.
/// Counting from `0` makes it a list, and a `$count` that cannot be `0`
/// leaves it non-empty.
/// How many entries a literal `array_fill()` builds into a shape; a longer
/// one is a table, typed as a list of the value.
const MAX_FILLED_ENTRIES: i64 = 64;

fn array_fill_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    use crate::type_engine::type_ops::int_interval::IntInterval;

    if (0..3).any(|index| args.is_spread(index) || args.is_named(index)) {
        return None;
    }
    let value = args.arg_raw_type(2)?;
    let start = args
        .arg_raw_type(0)
        .and_then(|start| start.as_literal().and_then(LiteralValue::parse_i64));
    // A literal start and count give the shape itself, its keys counting
    // up from the start (from a negative one too, since PHP 8).
    if let Some(start) = start
        && let Some(count) = args
            .arg_raw_type(1)
            .and_then(|count| count.as_literal().and_then(LiteralValue::parse_i64))
        && (0..=MAX_FILLED_ENTRIES).contains(&count)
    {
        let entries = (0..count)
            .map(|i| ShapeEntry {
                key: Some((start + i).to_string()),
                value_type: value.clone(),
                optional: false,
            })
            .collect();
        return Some(shape_with_implicit_keys(entries));
    }
    let from_zero = start == Some(0);
    let filled = if from_zero {
        PhpType::list(value)
    } else {
        PhpType::generic_array(PhpType::int(), value)
    };
    let non_empty = args
        .arg_raw_type(1)
        .and_then(|count| IntInterval::of_int(&count))
        .is_some_and(|count| count.min.is_some_and(|min| min >= 1));
    Some(if non_empty {
        filled.non_empty_array_form()
    } else {
        filled
    })
}

/// The largest (or smallest) of `values` when every one is an `int` or
/// `float` literal. `None` when any is not, or there are none.
fn extreme_number<'a>(
    values: impl Iterator<Item = &'a PhpType>,
    want_max: bool,
) -> Option<PhpType> {
    let mut best: Option<(f64, &PhpType)> = None;
    for value in values {
        let n = match value.as_literal()? {
            literal @ LiteralValue::Int(_) => literal.parse_i64()? as f64,
            literal @ LiteralValue::Float(_) => literal.parse_f64()?,
            LiteralValue::String(_) => return None,
        };
        let better = best.is_none_or(|(b, _)| if want_max { n > b } else { n < b });
        if better {
            best = Some((n, value));
        }
    }
    best.map(|(_, value)| value.clone())
}

/// The range `max()` (or `min()`) of integer arguments lands in, so
/// `max(1, $n)` is `int<1, max>` for an `int $n`. `None` when any argument
/// may be something other than an integer.
fn extreme_int_range(args: &[PhpType], want_max: bool) -> Option<PhpType> {
    use crate::type_engine::type_ops::int_interval::IntInterval;

    // `null` compares below every int but zero, so once another argument
    // is at least one it can never be `max()`'s answer.
    let null_loses = want_max
        && args.iter().any(|arg| {
            arg.union_members()
                .into_iter()
                .map(IntInterval::of_int)
                .collect::<Option<Vec<_>>>()
                .is_some_and(|members| {
                    IntInterval::hull_of(&members)
                        .min
                        .is_some_and(|min| min >= 1)
                })
        });
    let mut intervals = args.iter().map(|arg| {
        let mut members = Vec::new();
        for member in arg.union_members() {
            let member = match member.kind() {
                TypeKind::Nullable(inner) if null_loses => inner,
                _ => member,
            };
            if null_loses && member.is_null() {
                continue;
            }
            members.push(IntInterval::of_int(member)?);
        }
        (!members.is_empty()).then(|| IntInterval::hull_of(&members))
    });
    let first = intervals.next()??;
    // A `None` bound is unbounded, so on the side the extreme moves
    // towards it wins, and on the other side it loses.
    let toward = |a: Option<i64>, b: Option<i64>| {
        a.zip(b)
            .map(|(a, b)| if want_max { a.max(b) } else { a.min(b) })
    };
    let away = |a: Option<i64>, b: Option<i64>| match (a, b) {
        (Some(a), Some(b)) => Some(if want_max { a.max(b) } else { a.min(b) }),
        (bound, None) | (None, bound) => bound,
    };
    let extreme = intervals.try_fold(first, |acc, interval| {
        let interval = interval?;
        Some(if want_max {
            IntInterval {
                min: away(acc.min, interval.min),
                max: toward(acc.max, interval.max),
            }
        } else {
            IntInterval {
                min: toward(acc.min, interval.min),
                max: away(acc.max, interval.max),
            }
        })
    })?;
    Some(extreme.into_type())
}

/// Widen literal `int`/`float`/`string` members the way
/// [`PhpType::widen_scalar_literals`] does, but leave `true`/`false` alone.
///
/// `widen_scalar_literals` folds both halves of a boolean literal into
/// `bool` because it targets a mutable-collection boundary (an array
/// element, a stored property) where tracking which exact literal arrived
/// stops being useful. `max()`/`min()` hand back one of their arguments
/// verbatim instead of storing it, so a lone `false` argument (`int|false`
/// from `filemtime()`) must stay `false` rather than degrade to `bool`: a
/// later `!== false` check or `?:` fallback depends on the narrower type.
fn widen_non_bool_scalar_literals(ty: &PhpType) -> PhpType {
    match ty.kind() {
        TypeKind::Literal(value) => match &**value {
            LiteralValue::Int(_) => PhpType::int(),
            LiteralValue::Float(_) => PhpType::float(),
            LiteralValue::String(_) => PhpType::string(),
        },
        TypeKind::Union(members) => {
            PhpType::union(members.iter().map(widen_non_bool_scalar_literals).collect())
        }
        TypeKind::Nullable(inner) => PhpType::nullable(widen_non_bool_scalar_literals(inner)),
        _ => ty.clone(),
    }
}

/// The key type an array-like carries, as a domain a caller may report
/// verbatim.
///
/// [`PhpType::iterable_key_type`] answers `int` for the shapes that name
/// only a value type (`array<T>`, `T[]`, bare `array`), which is the useful
/// guess for iteration but an over-claim for anything reporting a key back
/// to the user. Those come back as `None` here so the caller can decline
/// rather than invent an `int` the argument never promised.
pub(crate) fn array_key_domain(ty: &PhpType) -> Option<PhpType> {
    (!ty.has_open_key_domain())
        .then(|| ty.iterable_key_type())
        .flatten()
}

/// The container `array_map` returns around `element`.
///
/// Handed one array, PHP runs the callback over each entry and keeps the
/// keys; handed several it zips them into a fresh list.
fn array_map_container(element: PhpType, args: &dyn ArrayFuncArgs) -> PhpType {
    if args.has_arg(2) {
        return PhpType::list(element);
    }
    let Some(input) = args.arg_raw_type(1).map(|ty| ty.generalized_array()) else {
        return PhpType::list(element);
    };
    // A `list` input keeps its `0, 1, 2, …` keys, which is more than the
    // `array<int, T>` its key type alone would say.
    if is_list_type(&input) {
        return PhpType::list(element);
    }
    // Keys are carried over untouched, so an input that never named its key
    // type produces a result that cannot name one either. Answering
    // `array<int, T>` (or `list<T>`) here would invent the `0, 1, 2, …` an
    // `array<T>` input never promised, and the invention then contradicts
    // whatever the caller assigns the result to.
    match array_key_domain(&input) {
        Some(key) => PhpType::generic_array(key, element),
        None => PhpType::generic_array_val(element),
    }
}

/// `array_map` over a single shape: every key survives and each value
/// becomes what the callback returns for it, so `array{c: string}` maps to
/// `array{c: T}` and a `list{…}` keeps its arity.
///
/// A short shape hands the callback each entry's own value, literals and
/// all, so `array_map(fn ($k) => ['k' => $k], ['a', 'b'])` keeps `'a'` and
/// `'b'`. In a longer one, entries whose values widen to the same type
/// share one callback inference, so a long homogeneous literal costs no
/// more than a list.
fn array_map_shape(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if args.has_arg(2) || args.is_spread(1) {
        return None;
    }
    let input = args.arg_raw_type(1)?;
    let TypeKind::ArrayShape(entries) = input.kind() else {
        return None;
    };
    let exact = entries.len() <= MAX_EXACT_MAPPED_ENTRIES;
    let mut mapped_by_seed: Vec<(PhpType, PhpType)> = Vec::new();
    let mut mapped = Vec::with_capacity(entries.len());
    for entry in entries.iter() {
        let seed = if exact {
            entry.value_type.clone()
        } else {
            entry.value_type.widen_scalar_literals()
        };
        let value = match mapped_by_seed.iter().find(|(s, _)| *s == seed) {
            Some((_, value)) => value.clone(),
            None => {
                let result = array_map_element_type(args, Some(seed.clone()))?;
                let value = if exact {
                    if result.is_void() {
                        PhpType::null()
                    } else {
                        result
                    }
                } else {
                    array_map_value(result)
                };
                mapped_by_seed.push((seed, value.clone()));
                value
            }
        };
        mapped.push(ShapeEntry {
            value_type: value,
            ..entry.clone()
        });
    }
    Some(if matches!(input.raw_kind(), TypeKind::ListShape(_)) {
        PhpType::list_shape(mapped)
    } else {
        PhpType::array_shape(mapped)
    })
}

/// How many entries a shape may have for `array_map` to run the callback
/// over each one's exact value.
const MAX_EXACT_MAPPED_ENTRIES: usize = 16;

/// A callback result as the value `array_map` stores: literals widened,
/// and a `void` callback's missing return read as the `null` it produces.
fn array_map_value(result: PhpType) -> PhpType {
    if result.is_void() {
        return PhpType::null();
    }
    result.widen_scalar_literals()
}

/// Whether `ty` promises the sequential integer keys a `list` has.
fn is_list_type(ty: &PhpType) -> bool {
    match ty.raw_kind() {
        TypeKind::Named(name) => crate::php_type::is_list_name(name.as_str()),
        TypeKind::Generic(g) => crate::php_type::is_list_name(g.name.as_str()),
        TypeKind::ListShape(_) => true,
        TypeKind::Union(members) => members.iter().all(is_list_type),
        _ => false,
    }
}

/// The container `array_filter` hands back for an input of type `raw`.
///
/// The filter keeps the key of every entry it keeps, so the numbering of
/// a filtered `list` comes back with gaps: `array_filter([3, 4, 5], fn
/// ($v) => $v > 3)` starts at key 1, and reading `[0]` off it finds
/// nothing. The result is `array<int, T>`, which is what `array_values()`
/// exists to renumber. A filter may also drop every entry, so a
/// `non-empty-` refinement does not survive the call either.
///
/// Everything else is returned unchanged: an `array<string, T>` really
/// does keep its `string` keys.
fn filtered_container(raw: &PhpType) -> PhpType {
    match raw.kind() {
        TypeKind::Generic(g) if crate::php_type::is_list_name(g.name.as_str()) => {
            match g.args.first() {
                Some(value) => PhpType::generic_array(PhpType::int(), value.clone()),
                None => PhpType::array(),
            }
        }
        TypeKind::Generic(g) if crate::php_type::is_non_empty_array_name(g.name.as_str()) => {
            PhpType::generic("array", g.args.clone())
        }
        TypeKind::Union(members) => {
            PhpType::union(members.iter().map(filtered_container).collect())
        }
        TypeKind::Nullable(inner) => PhpType::nullable(filtered_container(inner)),
        _ => raw.clone(),
    }
}

/// Rebuild an iterable type with its element narrowed to the members that
/// pass a truthiness test.
///
/// Returns `None` when the element type has no falsy member to remove (so
/// the caller keeps the type it already has) or when every member is falsy,
/// since `array<string, never>` is a worse answer than the original.
fn filter_element_type(raw: &PhpType) -> Option<PhpType> {
    let element = raw.extract_value_type(false)?;
    let truthy = element.truthy_type()?;
    if truthy == *element {
        return None;
    }
    with_element_type(raw, truthy)
}

/// Rebuild an iterable type around a new element type, keeping the
/// container it already names.
pub(crate) fn with_element_type(raw: &PhpType, element: PhpType) -> Option<PhpType> {
    match raw.kind() {
        TypeKind::Array(_) => Some(PhpType::array_of(element)),
        TypeKind::Generic(g) if !g.args.is_empty() => {
            let mut args = g.args.clone();
            // Same `<TKey, TValue>` convention `extract_value_type` reads:
            // the value is the second argument when there are two or more,
            // and the lone argument otherwise (`list<V>`).
            let value_idx = if args.len() >= 2 { 1 } else { args.len() - 1 };
            args[value_idx] = element;
            Some(PhpType::generic_atom(g.name, args))
        }
        _ => None,
    }
}

/// Rebuild an `array_filter` result with what its callback proves about
/// the entries it keeps.
///
/// The callback is handed the value, the key, or both depending on the
/// mode argument, and each narrows the half it arrives in. Returns
/// `None` when neither is narrowed.
fn filter_callback_type(raw: &PhpType, args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let narrowed_value = filter_value_type(raw, args);
    let base = narrowed_value.as_ref().unwrap_or(raw);
    filter_key_type(base, args).or(narrowed_value)
}

/// Rebuild an `array_filter` result with its element type narrowed to
/// what the callback asserts about the value it was handed.
///
/// Returns `None` unless the call runs in one of the two modes that pass
/// the value (the default and `ARRAY_FILTER_USE_BOTH`) and the callback
/// proves something the element type does not already say.
fn filter_value_type(raw: &PhpType, args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let param_index = filter_value_param_index(args)?;
    let element = raw.extract_value_type(false)?;
    let narrowed = filter_callback_narrowing(args, param_index, element)?;
    // A callback that admits every value it could receive says nothing,
    // and the rebuilt union would only reorder the members.
    if element.is_subtype_of(&narrowed) {
        return None;
    }
    with_element_type(raw, narrowed)
}

/// Rebuild an `array_filter` result with its key type narrowed to what
/// the callback asserts about the key it was handed.
///
/// Returns `None` unless the call runs in one of the two modes that pass
/// the key (`ARRAY_FILTER_USE_KEY`, `ARRAY_FILTER_USE_BOTH`), the callback
/// proves something about it, and the input carries a key type the proof
/// can narrow.
fn filter_key_type(raw: &PhpType, args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let param_index = filter_key_param_index(args)?;
    // `array<string>` and `string[]` name a value type and say nothing about
    // their keys, so the callback narrows every key PHP permits. Reading the
    // `int` that iteration assumes for them would leave `is_string($k)` with
    // nothing to keep and drop the narrowing entirely.
    let key = if raw.has_open_key_domain() {
        PhpType::union(vec![PhpType::int(), PhpType::string()])
    } else {
        raw.iterable_key_type()?
    };
    let narrowed = filter_callback_narrowing(args, param_index, &key)?;
    // A callback that admits every key it could receive (`is_int($k) ||
    // is_string($k)`) leaves nothing to say, and answering with the
    // rebuilt union would only reorder its members.
    if key.is_subtype_of(&narrowed) {
        return None;
    }
    let value = raw.extract_value_type(false)?.clone();
    // A filter can drop every entry, so the result is a plain `array`
    // whatever refinement (`non-empty-array`, `list`) the input carried.
    match raw.kind() {
        TypeKind::Array(_) => Some(PhpType::generic_array(narrowed, value)),
        TypeKind::Generic(g) if is_array_like_name(g.name.as_str()) => {
            Some(PhpType::generic_array(narrowed, value))
        }
        _ => None,
    }
}

/// Which of the callback's parameters receives the value, from
/// `array_filter`'s mode argument.
///
/// The default mode passes the value alone, and `ARRAY_FILTER_USE_BOTH`
/// passes it ahead of the key. `ARRAY_FILTER_USE_KEY` never shows the
/// callback a value, and a mode written as anything this cannot read
/// might be that one.
fn filter_value_param_index(args: &dyn ArrayFuncArgs) -> Option<usize> {
    if !args.has_arg(2) {
        return Some(0);
    }
    match args.arg_atom_text(2)?.as_str() {
        "ARRAY_FILTER_USE_BOTH" | "1" | "0" => Some(0),
        _ => None,
    }
}

/// Which of the callback's parameters receives the key, from
/// `array_filter`'s mode argument.
///
/// The default mode passes only the value, so the callback says nothing
/// about the keys and this returns `None`.
fn filter_key_param_index(args: &dyn ArrayFuncArgs) -> Option<usize> {
    match args.arg_atom_text(2)?.as_str() {
        "ARRAY_FILTER_USE_KEY" | "2" => Some(0),
        // `ARRAY_FILTER_USE_BOTH` passes the value first and the key
        // second.
        "ARRAY_FILTER_USE_BOTH" | "1" => Some(1),
        _ => None,
    }
}

/// Extract the output element type for `array_map($callback, $array)`.
///
/// Strategy:
/// 1. If the callback (first arg) is a closure/arrow function with a
///    return type hint, use that — narrowed by what the body returns,
///    since a callback may declare a supertype of what it produces
///    (`fn (X $p): MethodReflection => $p->getTransformedMethod()` really
///    returns the `ExtendedMethodReflection` the body hands back).
/// 2. Otherwise infer it from the callback body, with the callback's
///    first parameter seeded to the input array's element type.
/// 3. Otherwise use the return type the callback's own type declares
///    (a `callable(…): T` or `Closure(…): T` value).
/// 4. Otherwise assume the callback passes its element through.
///
/// `input_element` is the value the callback is handed: the input's
/// element type, or one shape entry's value.
fn array_map_element_type(
    args: &dyn ArrayFuncArgs,
    input_element: Option<PhpType>,
) -> Option<PhpType> {
    if let Some(declared) = args.callback_declared_return_type(0)
        && !declared.is_untyped()
    {
        let seed = input_element.unwrap_or_else(PhpType::mixed);
        let narrowed = args
            .callback_inferred_return_type(0, &seed)
            .filter(|inferred| args.narrows(inferred, &declared));
        return Some(narrowed.unwrap_or(declared));
    }

    if let Some(inferred) = input_element
        .as_ref()
        .and_then(|element| args.callback_inferred_return_type(0, element))
    {
        return Some(inferred);
    }

    if let Some(returned) = args
        .arg_raw_type(0)
        .as_ref()
        .and_then(PhpType::callable_return_type)
        .filter(|ty| !ty.is_untyped() && !ty.is_mixed())
    {
        return Some(returned.clone());
    }

    // Final fallback: assume the callback passes its element through. That
    // only holds for element types a callback is unlikely to convert; a
    // scalar element says nothing about the result, and `array_map('intval',
    // $strings)` would be reported as `list<string>` on the strength of an
    // input the callback exists to change.
    input_element.filter(|element| !element.is_scalar_leaf())
}

/// `count($array)` of an array whose size its type fixes: the number of
/// entries of a shape, or the range its alternatives and optional entries
/// span.
/// `None` for anything else, which keeps the stub's `int<0, max>`.
///
/// A mode other than a written `COUNT_NORMAL` may count recursively, which
/// adds the entries of nested arrays, so it gives the shape's size only
/// when no entry can hold one.
fn count_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if args.is_spread(0) || args.is_named(0) || args.is_spread(1) || args.is_named(1) {
        return None;
    }
    let recursive =
        args.has_arg(1) && !matches!(args.arg_atom_text(1).as_deref(), Some("COUNT_NORMAL" | "0"));
    let raw = args.arg_raw_type(0)?;
    let mut sizes: Vec<(usize, usize)> = Vec::new();
    for member in raw.union_members() {
        if member.as_unsealed_shape().is_some() {
            return None;
        }
        let TypeKind::ArrayShape(entries) = member.kind() else {
            return None;
        };
        if recursive
            && entries
                .iter()
                .any(|entry| !is_never_an_array(&entry.value_type))
        {
            return None;
        }
        let required = entries.iter().filter(|entry| !entry.optional).count();
        sizes.push((required, entries.len()));
    }
    let min = sizes.iter().map(|(min, _)| *min).min()?;
    let max = sizes.iter().map(|(_, max)| *max).max()?;
    if min == max {
        return Some(PhpType::literal_int(min.to_string()));
    }
    Some(PhpType::int_range(min.to_string(), max.to_string()))
}

/// Whether no value of `ty` is an array, so a recursive `count()` skips it.
pub(in crate::type_engine) fn is_never_an_array(ty: &PhpType) -> bool {
    ty.union_members().iter().all(|member| {
        member.is_int_subtype()
            || member.is_string_subtype()
            || member.is_float()
            || matches!(member.as_literal(), Some(LiteralValue::Float(_)))
            || member.is_bool()
            || member.is_true()
            || member.is_false()
            || member.is_null()
            // `count()` does not look inside an object, `Countable` or not.
            || (member.is_object_like() && !member.is_array_like() && !member.is_iterable())
    })
}
