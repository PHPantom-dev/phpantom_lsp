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
use crate::php_type::PhpType;

use super::{ARRAY_ELEMENT_FUNCS, ARRAY_PRESERVING_FUNCS};

/// Argument access needed by the array-function return-type rules.
pub(in crate::type_engine) trait ArrayFuncArgs {
    /// The unflattened type of the argument at `index` (e.g.
    /// `list<User>`), or `None` when it cannot be resolved.
    fn arg_raw_type(&self, index: usize) -> Option<PhpType>;

    /// Whether the argument at `index` is the literal `false`.
    fn is_false_literal(&self, index: usize) -> bool;

    /// The return type declared on the closure or arrow function at
    /// `index`.  `None` when the argument is not an inline function or
    /// declares no return type.
    fn callback_declared_return_type(&self, index: usize) -> Option<PhpType>;

    /// The return type inferred from the body of the closure or arrow
    /// function at `index`, with its first parameter seeded to
    /// `param_type`.
    fn callback_inferred_return_type(&self, index: usize, param_type: &PhpType) -> Option<PhpType>;
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
    // Type-preserving functions: output array has same element type.
    if ARRAY_PRESERVING_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        let raw = args.arg_raw_type(0)?;
        // If the raw type already has generic params, return it as-is
        // so downstream `PhpType::extract_value_type` can extract the
        // element type.  Otherwise it's a plain class name and we
        // can't infer element type.
        if raw.extract_value_type(true).is_some() {
            return Some(raw);
        }
    }

    // array_map: callback is first arg, array is second.
    // The callback's return type determines the output element type.
    if func_name.eq_ignore_ascii_case("array_map")
        && let Some(element_type) = array_map_element_type(args)
    {
        return Some(PhpType::list(element_type.widen_scalar_literals()));
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
        if args.is_false_literal(1) {
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
    if ARRAY_ELEMENT_FUNCS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(func_name))
    {
        return args.arg_raw_type(0)?.extract_value_type(true).cloned();
    }

    None
}

/// Extract the output element type for `array_map($callback, $array)`.
///
/// Strategy:
/// 1. If the callback (first arg) is a closure/arrow function with a
///    return type hint, use that.
/// 2. Otherwise infer it from the callback body, with the callback's
///    first parameter seeded to the input array's element type.
/// 3. Otherwise assume the callback passes its element through.
fn array_map_element_type(args: &dyn ArrayFuncArgs) -> Option<PhpType> {
    if let Some(declared) = args.callback_declared_return_type(0)
        && !declared.is_untyped()
    {
        return Some(declared);
    }

    let input_element = args.arg_raw_type(1)?.extract_element_type()?.clone();

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
