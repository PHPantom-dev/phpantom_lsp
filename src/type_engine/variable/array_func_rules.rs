/// Return-type rules for the array-producing and element-extracting
/// standard library functions.
///
/// The rules themselves live here, independent of where the call's
/// arguments come from.  Two callers reach a call expression from
/// opposite directions and both need the same answers:
///
/// * the AST walker, when the call is an assignment right-hand side and
///   a parsed `ArgumentList` is in hand (see
///   [`super::raw_type_inference`]), and
/// * the text-driven call resolver, when the call appears inline (as a
///   subject, an array-access base, or another call's argument) and only
///   the raw argument source text is available (see
///   `type_engine::call_resolution`).
///
/// [`ArrayFuncArgs`] is the seam between the two: each caller answers
/// the handful of questions the rules ask about an argument, and the
/// rules stay in one place so a fix to `array_map`'s element type
/// reaches every consumer.
use crate::php_type::{LiteralValue, PhpType, TypeKind, is_array_like_name};

use super::{ARRAY_ELEMENT_FUNCS, ARRAY_PRESERVING_FUNCS};

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
            return Some(raw);
        }
    }

    // `array_chunk` is the one splitter that adds a level of nesting rather
    // than rearranging entries: the result's elements are arrays of the
    // input's elements. The chunks are renumbered from zero unless
    // `$preserve_keys` asks for the original keys back.
    if func_name.eq_ignore_ascii_case("array_chunk") {
        let raw = args.arg_raw_type(0)?.generalized_array();
        let value = raw.extract_value_type(false)?.clone();
        let chunk = if args.bool_literal(2) == Some(true) {
            match array_key_domain(&raw) {
                Some(key) => PhpType::generic_array(key, value),
                None => PhpType::generic_array_val(value),
            }
        } else {
            PhpType::list(value)
        };
        // The outer array is always renumbered, whatever the inner keys do.
        return Some(PhpType::list(chunk));
    }

    // `range` builds its elements from the bounds rather than from an array
    // it was handed, and a single fractional bound makes the whole range
    // fractional. That is a question about every argument at once, which the
    // conditional return type in `stub_patches` cannot ask.
    if func_name.eq_ignore_ascii_case("range") {
        return range_type(args);
    }

    // array_map: callback is first arg, array is second.
    // The callback's return type determines the output element type.
    if func_name.eq_ignore_ascii_case("array_map") {
        let element = array_map_element_type(args)?.widen_scalar_literals();
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
            .and_then(|key| super::resolution::normalize_array_key_type(&key));
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

    if ARRAY_ELEMENT_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        // A scalar element is the honest answer for `array_pop(list<string>)`
        // just as `User` is for `list<User>`, so the element type is read
        // without `skip_scalar`.
        return args.arg_raw_type(0)?.iterable_element_type();
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
        return min_max_type(args);
    }

    // The key readers are stubbed `TKey|null` because an empty array has no
    // key to report. An argument that proves it has entries rules that out.
    if func_name.eq_ignore_ascii_case("array_key_first")
        || func_name.eq_ignore_ascii_case("array_key_last")
        || func_name.eq_ignore_ascii_case("key")
    {
        let raw = args.arg_raw_type(0)?;
        return raw
            .is_provably_non_empty()
            .then(|| array_key_domain(&raw))
            .flatten();
    }

    if matches!(func_name, "array_sum" | "array_product") {
        // An array nobody typed the elements of decides neither half, and
        // the sum is then exactly what adding two `mixed` values is: the
        // pair, benevolent, because it is a gap in what the code said
        // rather than a value measured to be two things. Enforcing both
        // halves reported `array_sum($bytes)` handed to an `int` parameter
        // as a mismatch on code that is fine. PHPStan reaches the same
        // answer by summing the element type with itself.
        let undecided = || {
            Some(PhpType::benevolent(PhpType::union(vec![
                PhpType::int(),
                PhpType::float(),
            ])))
        };
        let Some(element) = args.arg_raw_type(0).and_then(|a| a.iterable_element_type()) else {
            return undecided();
        };
        let members: Vec<&PhpType> = match element.kind() {
            TypeKind::Union(m) => m.iter().collect(),
            _ => vec![&element],
        };
        if members.iter().any(|m| m.is_mixed()) {
            return undecided();
        }
        let (int_ty, float_ty) = (PhpType::int(), PhpType::float());
        let all_int = members.iter().all(|m| m.is_subtype_of(&int_ty));
        if all_int {
            return Some(int_ty);
        }
        // `int` is a subtype of `float` here (PHP widens it silently), so
        // a member has to be tested against both to tell `list<float>`
        // apart from `list<int|float>` — the latter really can sum to
        // either and keeps the declared union.
        if members
            .iter()
            .all(|m| m.is_subtype_of(&float_ty) && !m.is_subtype_of(&int_ty))
        {
            return Some(float_ty);
        }
        return None;
    }

    None
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

/// The list a numeric `range()` builds.
///
/// PHP walks from `$start` to `$end` in `$step`s, and the result is integral
/// only when all three are: one fractional bound (or step) makes every element
/// a float. So the answer is decided by the bounds *together*, and a single
/// argument that cannot be typed leaves the whole call undecided.
///
/// Declining hands the call back to the stub's own
/// `($start is string ? list<string> : list<int|float>)`, which is the right
/// answer both for a character range and for a numeric one whose bounds are
/// not known — so this rule only has to recognise the two it can prove.
///
/// `int` is a subtype of `float` here (PHP widens it silently), so the
/// integral case has to be tested first: `list<int>` would otherwise read as
/// an all-float range.
fn range_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    let mut bounds = vec![args.arg_raw_type(0)?, args.arg_raw_type(1)?];
    if args.has_arg(2) {
        bounds.push(args.arg_raw_type(2)?);
    }
    let int_ty = PhpType::int();
    if bounds.iter().all(|b| b.is_subtype_of(&int_ty)) {
        return Some(PhpType::list(int_ty));
    }
    let float_ty = PhpType::float();
    bounds
        .iter()
        .all(|b| b.is_subtype_of(&float_ty))
        .then(|| PhpType::list(float_ty))
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
    let int_ty = PhpType::int();
    let mut values: Vec<PhpType> = Vec::new();
    let mut keys: Vec<PhpType> = Vec::new();
    let mut open_keys = false;
    let mut index = 0;
    while args.has_arg(index) {
        if args.is_spread(index) {
            return None;
        }
        let raw = args.arg_raw_type(index)?;
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

/// The type of `max()`/`min()`'s result.
///
/// PHP gives the function two shapes: handed a single iterable it compares
/// the entries and returns one of them, and handed several values it
/// compares those. Either way the result is one of the values that went in,
/// so the answer is a member of what was passed rather than a new type.
///
/// Returns `None` as soon as one argument cannot be resolved, since a union
/// missing a member would be narrower than the call can promise.
fn min_max_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if !args.has_arg(1) {
        return args.arg_raw_type(0)?.iterable_element_type();
    }

    let mut members = Vec::new();
    let mut index = 0;
    while args.has_arg(index) {
        let widened = widen_non_bool_scalar_literals(&args.arg_raw_type(index)?);
        for member in widened.union_members() {
            if !members.contains(member) {
                members.push(member.clone());
            }
        }
        index += 1;
    }
    match members.len() {
        0 => None,
        1 => members.pop(),
        _ => Some(PhpType::union(members)),
    }
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
    let narrowed = args.callback_param_narrowing(1, param_index, element)?;
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
    let narrowed = args.callback_param_narrowing(1, param_index, &key)?;
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
/// 3. Otherwise assume the callback passes its element through.
fn array_map_element_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if let Some(declared) = args.callback_declared_return_type(0)
        && !declared.is_untyped()
    {
        // Only a class-like declaration can be narrowed by a hierarchy
        // relation, so a scalar one skips the body walk entirely.
        if declared.is_scalar_leaf() {
            return Some(declared);
        }
        let seed = args
            .arg_raw_type(1)
            .and_then(|t| t.iterable_element_type())
            .unwrap_or_else(PhpType::mixed);
        let narrowed = args
            .callback_inferred_return_type(0, &seed)
            .filter(|inferred| args.narrows(inferred, &declared));
        return Some(narrowed.unwrap_or(declared));
    }

    let input_element = args.arg_raw_type(1)?.iterable_element_type()?;

    if let Some(inferred) = args.callback_inferred_return_type(0, &input_element) {
        return Some(inferred);
    }

    // Final fallback: assume the callback passes its element through. That
    // only holds for element types a callback is unlikely to convert; a
    // scalar element says nothing about the result, and `array_map('intval',
    // $strings)` would be reported as `list<string>` on the strength of an
    // input the callback exists to change.
    (!input_element.is_scalar_leaf()).then_some(input_element)
}
