//! Patches for the functions phpstorm-stubs declares incompletely.

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::FunctionInfo;

/// Apply all registered stub patches to a freshly-parsed function.
///
/// Called from [`find_or_load_function`](crate::resolution) after a
/// `FunctionInfo` is parsed from embedded phpstorm-stubs, before it is
/// cached in `global_functions`.  Only functions with known deficiencies
/// are patched; all others pass through unchanged.
pub fn apply_function_stub_patches(func: &mut FunctionInfo) {
    match func.name.as_str() {
        "range" => patch_range(func),
        "str_word_count" => patch_str_word_count(func),
        "stream_bucket_make_writeable" => patch_stream_bucket_make_writeable(func),
        "array_map" => patch_array_map(func),
        "array_filter" => patch_array_filter(func),
        "usort" | "uasort" => patch_user_sort(func, SortComparand::Value),
        "uksort" => patch_user_sort(func, SortComparand::Key),
        "array_fill_keys" => patch_array_fill_keys(func),
        "array_keys" => patch_array_key_value_generics(func, "$array", "list<TKey>"),
        "array_values" => patch_array_key_value_generics(func, "$array", "list<TValue>"),
        "array_search" => patch_array_key_value_generics(func, "$haystack", "TKey|false"),
        "array_key_first" | "array_key_last" => {
            patch_array_key_value_generics(func, "$array", "TKey|null")
        }
        "key" => patch_array_key_value_generics(func, "$array", "TKey|null"),
        "pathinfo" => patch_pathinfo(func),
        "print_r" => patch_print_r(func),
        "hrtime" => patch_hrtime(func),
        "microtime" => patch_microtime(func),
        "getenv" => patch_getenv(func),
        "func_get_args" => func.return_type = Some(PhpType::list(PhpType::mixed())),
        "mb_convert_encoding" => patch_mb_convert_encoding(func),
        "abs" => patch_abs(func),
        "var_export" => patch_var_export(func),
        "mb_internal_encoding" => patch_mb_internal_encoding(func),
        "version_compare" => patch_version_compare(func),
        "sscanf" => patch_scanf_family(func, "int", "array|null"),
        "fscanf" => patch_scanf_family(func, "int|false", "array|false|null"),
        "array_reduce" => patch_array_reduce(func),
        "pow" => patch_pow(func),
        "ini_get" => patch_ini_get(func),
        "get_class" => patch_get_class(func),
        "preg_replace"
        | "preg_replace_callback"
        | "preg_replace_callback_array"
        | "preg_filter" => patch_replace_family(func, "$subject", true),
        "str_replace" | "str_ireplace" => patch_replace_family(func, "$subject", false),
        "substr_replace" => patch_replace_family(func, "$string", false),
        "spl_autoload_register" => patch_spl_autoload_register(func),
        "define" => widen_parameter_to_mixed(func, "$value"),
        name if name.starts_with("ctype_") => widen_parameter_to_mixed(func, "$text"),
        _ => {}
    }
    if crate::benevolent_builtins::function_is_benevolent(&func.name) {
        mark_benevolent(&mut func.return_type);
    }
}

/// Tag a return type as one whose failure branch is not worth enforcing.
///
/// The type is unchanged in every other respect — see
/// [`crate::benevolent_builtins`] — and a return type that is not a union
/// on this PHP version comes back untagged.
fn mark_benevolent(return_type: &mut Option<PhpType>) {
    if let Some(ty) = return_type.take() {
        *return_type = Some(PhpType::benevolent(ty));
    }
}

/// Widen a parameter the stubs type more narrowly than php-src does.
///
/// `ctype_digit()` and its siblings take `mixed $text` (passing an int is a
/// deprecation, not a type error), and `define()`'s `$value` has been `mixed`
/// since PHP 8.0, but the stubs keep the old `string` hint and the pre-7.0
/// `null|array|bool|int|float|string` `@param` tag respectively. Both the
/// docblock type and the native hint are widened so hover shows what the
/// function really accepts.
fn widen_parameter_to_mixed(func: &mut FunctionInfo, param_name: &str) {
    for param in func.parameters.make_mut() {
        if param.name == param_name {
            param.type_hint = Some(PhpType::mixed());
            param.native_type_hint = Some(PhpType::mixed());
        }
    }
}

/// Spell out `spl_autoload_register`'s autoloader signature.
///
/// phpstorm-stubs type the callback as bare `?callable`, so the closure
/// idiom `spl_autoload_register(function ($class) { … })` leaves
/// `$class` untyped and every string operation on it widens to the full
/// union its argument allows.  PHP always calls the autoloader with the
/// requested class name and ignores whatever it returns, which is
/// exactly `callable(string): void`.  The nullable wrapper stays: calling
/// `spl_autoload_register()` with no arguments is still valid.
fn patch_spl_autoload_register(func: &mut FunctionInfo) {
    // Expected stub shape: `spl_autoload_register(?callable $callback = null, …)`.
    if func
        .parameters
        .first()
        .is_none_or(|p| p.name.as_str() != "$callback")
    {
        return;
    }
    let hint = PhpType::parse("?callable(string): void");
    for param in func.parameters.make_mut() {
        if param.name.as_str() == "$callback" {
            param.type_hint = Some(hint.clone());
        }
    }
}

/// Link `array_map`'s callback parameter to the input array's element
/// type so a closure passed to it gets its parameter typed.
///
/// phpstorm-stubs declare the callback as bare `callable|null` and the
/// array as bare `array`, so `array_map(fn($x) => $x->foo(), $items)`
/// leaves `$x` untyped.  We add `@template TValue`, retype the callback
/// as `callable(TValue): mixed`, and the array as `array<TValue>`, then
/// bind `TValue` from the array argument.  The callback's *return* type
/// (and thus `array_map`'s own return) is still resolved by the
/// value-inspecting logic in `raw_type_inference.rs`, which this patch
/// leaves untouched by keeping the bare `array` return type.
fn patch_array_map(func: &mut FunctionInfo) {
    // Expected stub shape: `array_map(?callable $callback, array $array, …)`.
    let callback_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$callback" => p.name,
        _ => return,
    };
    let array_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$array" => p.name,
        _ => return,
    };
    link_callback_to_array_element(func, callback_name, array_name, "mixed");
}

/// Link `array_filter`'s callback parameter to the input array's element
/// type (the callback receives each element and its result is tested for
/// truthiness).
///
/// Unlike `array_map`, `array_filter` takes the array first and the
/// callback second: `array_filter(array $array, ?callable $callback, …)`.
///
/// The callback's return stays `mixed` because that is what PHP accepts:
/// `array_filter($items, fn ($i) => preg_match($re, $i))` keeps every
/// element the callback returns a truthy value for, and typing the
/// return as `bool` would call that idiom a type error.
fn patch_array_filter(func: &mut FunctionInfo) {
    let array_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$array" => p.name,
        _ => return,
    };
    let callback_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$callback" => p.name,
        _ => return,
    };
    link_callback_to_array_element(func, callback_name, array_name, "mixed");
}

/// Which half of the array a user-comparison sort hands its callback.
#[derive(Copy, Clone)]
enum SortComparand {
    /// `usort`/`uasort` compare values.
    Value,
    /// `uksort` compares keys.
    Key,
}

/// Spell out the comparison callback of `usort`, `uasort` and `uksort`.
///
/// phpstorm-stubs declare all three as
/// `usort(array &$array, callable $callback)`, so the comparison closure
/// idiom `usort($errors, fn ($a, $b) => $a->getLine() <=> $b->getLine())`
/// leaves both parameters untyped. PHP calls the callback with two
/// elements of the array it is sorting: two values for `usort`/`uasort`,
/// two keys for `uksort`. We add the `@template` pair, retype the array as
/// `array<TKey, TValue>` and the callback as `callable(T, T): int` over
/// whichever of the two it compares, then bind both from the array
/// argument. Mirrors PHPStan's own `stubs/arrayFunctions.stub`.
fn patch_user_sort(func: &mut FunctionInfo, comparand: SortComparand) {
    const TKEY: &str = "TKey";
    const TVALUE: &str = "TValue";

    // Expected stub shape: `usort(array &$array, callable $callback)`.
    let array_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$array" => p.name,
        _ => return,
    };
    let callback_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$callback" => p.name,
        _ => return,
    };

    let compared = match comparand {
        SortComparand::Value => TVALUE,
        SortComparand::Key => TKEY,
    };
    let array_hint = PhpType::parse(&format!("array<{TKEY}, {TVALUE}>"));
    let callback_hint = PhpType::parse(&format!("callable({compared}, {compared}): int"));

    for param in func.parameters.make_mut() {
        if param.name == array_name {
            param.type_hint = Some(array_hint.clone());
        } else if param.name == callback_name {
            param.type_hint = Some(callback_hint.clone());
        }
    }

    func.template_params = vec![atom(TKEY), atom(TVALUE)];
    // A bare `array` argument binds neither param, and `array-key` is
    // PHP's own answer for a key that nothing narrowed.
    func.template_param_bounds = [(atom(TKEY), PhpType::parse("array-key"))]
        .into_iter()
        .collect();
    // Only the array argument can bind them: the callback is the very
    // thing whose parameters these templates are there to type.
    func.template_bindings = vec![(atom(TKEY), array_name), (atom(TVALUE), array_name)];
}

/// Shared helper: give `func` a single `TValue` template bound from the
/// array parameter, and retype the callback as
/// `callable(TValue): <callback_return>` so a closure argument's first
/// parameter is inferred as the array's element type.
fn link_callback_to_array_element(
    func: &mut FunctionInfo,
    callback_name: crate::atom::Atom,
    array_name: crate::atom::Atom,
    callback_return: &str,
) {
    const TVALUE: &str = "TValue";
    let callback_hint = PhpType::parse(&format!("callable({}): {}", TVALUE, callback_return));
    let array_hint = PhpType::parse(&format!("array<{}>", TVALUE));

    for param in func.parameters.make_mut() {
        if param.name == callback_name {
            param.type_hint = Some(callback_hint.clone());
        } else if param.name == array_name {
            param.type_hint = Some(array_hint.clone());
        }
    }

    func.template_params = vec![atom(TVALUE)];
    func.template_param_bounds = Default::default();
    // Bind `TValue` from the array argument only.  The callback argument
    // (an unannotated closure) can't bind it, and listing it would just
    // add a no-op binding attempt.
    func.template_bindings = vec![(atom(TVALUE), array_name)];
}

/// Give a key- or value-returning array builtin the `@template` pair the
/// stubs leave off, so its return substitutes the caller's own generics.
///
/// phpstorm-stubs spell these out as concrete unions (`array_keys` returns
/// `int[]|string[]`, `array_search` returns `string|int|false`) or as a
/// bare `array`, which is the widest thing the signature can say without
/// generics. `array_flip` is the counter-example that shows the machinery
/// already works: it ships a real `@template` pair and resolves correctly
/// today, so the fix for the rest is to annotate them the same way rather
/// than to add per-function logic in Rust.
///
/// `array_param` is the parameter the generics bind from (`$haystack` for
/// `array_search`, `$array` for everything else) and `return_type` is
/// written in terms of `TKey`/`TValue`.
fn patch_array_key_value_generics(func: &mut FunctionInfo, array_param: &str, return_type: &str) {
    const TKEY: &str = "TKey";
    const TVALUE: &str = "TValue";

    let array_name = match func
        .parameters
        .iter()
        .find(|p| p.name.as_str() == array_param)
    {
        Some(p) => p.name,
        None => return,
    };

    let array_hint = PhpType::parse(&format!("array<{TKEY}, {TVALUE}>"));
    for param in func.parameters.make_mut() {
        if param.name == array_name {
            param.type_hint = Some(array_hint.clone());
        }
    }

    func.return_type = Some(PhpType::parse(return_type));
    func.template_params = vec![atom(TKEY), atom(TVALUE)];
    // A bare `array` argument binds neither param. `TKey` still has PHP's
    // own answer to fall back on — an array key is an `array-key` — which
    // beats the `mixed` an undeclared bound would leave behind.
    func.template_param_bounds = [(atom(TKEY), PhpType::parse("array-key"))]
        .into_iter()
        .collect();
    func.template_bindings = vec![(atom(TKEY), array_name), (atom(TVALUE), array_name)];
}

/// Give `array_fill_keys()` the generics that turn its two arguments into
/// the result's key and value types.
///
/// The stub is `array_fill_keys(array $keys, mixed $value): array`, so a
/// caller loses the one thing the call establishes: the keys of the result
/// are exactly the *values* of `$keys`. That matters downstream, because
/// `array_keys(array_fill_keys($names, true))` should hand back the
/// `$names` it started from rather than a bare `array-key`.
///
/// Unlike [`patch_array_key_value_generics`], `TKey` binds from the array
/// parameter's *element* type, not its key type.
fn patch_array_fill_keys(func: &mut FunctionInfo) {
    const TKEY: &str = "TKey";
    const TVALUE: &str = "TValue";

    let keys_name = match func.parameters.first() {
        Some(p) if p.name.as_str() == "$keys" => p.name,
        _ => return,
    };
    let value_name = match func.parameters.get(1) {
        Some(p) if p.name.as_str() == "$value" => p.name,
        _ => return,
    };

    let keys_hint = PhpType::parse(&format!("array<{TKEY}>"));
    let value_hint = PhpType::parse(TVALUE);
    for param in func.parameters.make_mut() {
        if param.name == keys_name {
            param.type_hint = Some(keys_hint.clone());
        } else if param.name == value_name {
            param.type_hint = Some(value_hint.clone());
        }
    }

    func.return_type = Some(PhpType::parse(&format!("array<{TKEY}, {TVALUE}>")));
    func.template_params = vec![atom(TKEY), atom(TVALUE)];
    // `$keys` whose element type is unknown leaves `TKey` on PHP's own
    // answer — whatever a `foreach` writes into an array is an `array-key`.
    func.template_param_bounds = [(atom(TKEY), PhpType::parse("array-key"))]
        .into_iter()
        .collect();
    func.template_bindings = vec![(atom(TKEY), keys_name), (atom(TVALUE), value_name)];
}

/// Patch `range()` to have a conditional return type.
///
/// phpstorm-stubs declare `range()` as returning bare `array`.
/// PHPStan infers `list<int>`, `list<float>`, or `list<string>` depending
/// on the argument types.  We approximate this with:
/// `($start is string ? list<string> : list<int|float>)`.
///
/// Splitting the numeric branch needs every bound at once — a single
/// fractional one makes the whole range fractional — which a conditional
/// keyed on one parameter cannot ask. That half lives in
/// `type_engine::variable::array_func_rules` and answers first; this
/// conditional is what a range it cannot pin down falls back to.
fn patch_range(func: &mut FunctionInfo) {
    func.conditional_return = Some(PhpType::conditional(
        "$start",
        false,
        PhpType::named(atom("string")),
        PhpType::list(PhpType::string()),
        PhpType::list(PhpType::union(vec![PhpType::int(), PhpType::float()])),
    ));
}

/// Patch `array_reduce()` to have a conditional return type keyed on
/// `$initial`.
///
/// The stub's `@return TCarry|null` covers the one case that really can
/// produce `null`: an empty array with no initial value. Handing the call an
/// initial value makes that the result instead, so the `null` branch cannot
/// happen and every reduction over a seeded accumulator carries a nullable it
/// never is.
fn patch_array_reduce(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$initial",
        PhpType::null(),
        PhpType::union(vec![PhpType::named(atom("TCarry")), PhpType::null()]),
        PhpType::named(atom("TCarry")),
    );
}

/// Give `get_class()` the generic that ties its result to the object it was
/// asked about.
///
/// The stub returns bare `string`, so the one thing the call establishes — the
/// result names *this* object's class — is lost the moment it is assigned.
/// PHPStan's stub declares `@template T of object` / `@param T $object` /
/// `@return class-string<T>`, which keeps `new ($className)` and
/// `$className::create()` resolvable and lets the result satisfy a
/// `class-string` parameter.
fn patch_get_class(func: &mut FunctionInfo) {
    const T: &str = "T";
    let Some(object) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    for param in func.parameters.make_mut() {
        if param.name == object {
            param.type_hint = Some(PhpType::named(atom(T)));
        }
    }
    func.return_type = Some(PhpType::parse("class-string<T>"));
    func.template_params = vec![atom(T)];
    // The no-argument form (deprecated, and removed in 8.4) asks about the
    // enclosing class, which the binding cannot see. `object` keeps that call
    // on `class-string<object>` — still a class-string, just not a specific
    // one.
    func.template_param_bounds = [(atom(T), PhpType::named(atom("object")))]
        .into_iter()
        .collect();
    func.template_bindings = vec![(atom(T), object)];
}

/// Patch `ini_get()` to have a conditional return type keyed on `$option`.
///
/// `false` means "no such directive", so an option PHP always defines cannot
/// produce it. The stubs declare `string|false` for every call, which puts a
/// failure branch on the `ini_get('memory_limit')` idiom that no amount of
/// checking can reach.
///
/// The list is PHPStan's (`IniGetReturnTypeExtension`): the core directives it
/// is willing to promise are always set. Anything outside it keeps the
/// declared union, since a directive an extension registers really can be
/// missing.
fn patch_ini_get(func: &mut FunctionInfo) {
    const ALWAYS_SET: [&str; 7] = [
        "date.timezone",
        "memory_limit",
        "max_memory_limit",
        "max_execution_time",
        "max_input_time",
        "default_socket_timeout",
        "precision",
    ];
    let Some(option) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    conditional_on(
        func,
        option.as_str(),
        PhpType::union(
            ALWAYS_SET
                .iter()
                .map(PhpType::literal_string_value)
                .collect(),
        ),
        PhpType::string(),
        PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
    );
}

/// Patch `pow()` to have a conditional return type keyed on its operands.
///
/// The `object` branch only exists for the operator-overloading extensions
/// (GMP, BCMath): raising two numbers to a power can only produce a number.
/// The stubs declare `object|int|float` for every call, so arithmetic on the
/// result of an ordinary `pow(2, $n)` is checked against a class.
/// An operand nobody typed decides nothing, and the union of both
/// branches would put `object` back into every such call — so both
/// conditionals here take the numeric branch unless an operand is
/// provably an object.
fn patch_pow(func: &mut FunctionInfo) {
    let numeric = PhpType::union(vec![PhpType::int(), PhpType::float()]);
    let object = PhpType::named(atom("object"));
    func.conditional_return = Some(PhpType::conditional_defaulting_to_else(
        "$num",
        false,
        object.clone(),
        object.clone(),
        PhpType::conditional_defaulting_to_else(
            "$exponent",
            false,
            object.clone(),
            object,
            numeric,
        ),
    ));
}

/// Patch `str_word_count()` to have a conditional return type.
///
/// phpstorm-stubs declare the flat union `string[]|int`, but the return type
/// is decided by `$format`: `0` (the default) counts the words, `1` lists
/// them, and `2` maps each word to the offset it starts at. A `$format` that
/// isn't a literal leaves the declared union, which is all the call site can
/// promise.
fn patch_str_word_count(func: &mut FunctionInfo) {
    let count = PhpType::int();
    let words = PhpType::list(PhpType::string());
    let words_by_offset = PhpType::generic_array(PhpType::int(), PhpType::string());
    let unknown_format = PhpType::union(vec![words.clone(), count.clone()]);

    func.conditional_return = Some(PhpType::conditional(
        "$format",
        false,
        PhpType::literal_int("0"),
        count,
        PhpType::conditional(
            "$format",
            false,
            PhpType::literal_int("1"),
            words,
            PhpType::conditional(
                "$format",
                false,
                PhpType::literal_int("2"),
                words_by_offset,
                unknown_format,
            ),
        ),
    ));
}

/// Give `func` a conditional return type keyed on one of its parameters.
///
/// Bails out when the stub does not declare that parameter: without it the
/// conditional would be decided against whichever argument happened to land
/// in slot 0, which is worse than the declared union.
fn conditional_on(
    func: &mut FunctionInfo,
    param_name: &str,
    condition: PhpType,
    then_type: PhpType,
    else_type: PhpType,
) {
    if !func.parameters.iter().any(|p| p.name == param_name) {
        return;
    }
    func.conditional_return = Some(PhpType::conditional(
        param_name, false, condition, then_type, else_type,
    ));
}

/// Patch `pathinfo()` to have a conditional return type keyed on `$flags`.
///
/// The stubs declare the flat union `string|array{…}`, but only the
/// all-elements form returns the array: any other flag asks for one component
/// and gets a `string` back. `PATHINFO_ALL` is the parameter's declared
/// default, so the one-argument call takes the array branch through the same
/// route an explicit `PATHINFO_ALL` does.
///
/// `extension` is optional in the shape because a path without a dot has no
/// extension key at all. Mirrors PHPStan's
/// `PathinfoFunctionDynamicReturnTypeExtension`.
fn patch_pathinfo(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$flags",
        PhpType::literal_int(PATHINFO_ALL.to_string()),
        PhpType::parse(
            "array{dirname: string, basename: string, extension?: string, filename: string}",
        ),
        PhpType::string(),
    );
}

/// `PATHINFO_ALL`, as defined by PHP's `ext/standard/string.h`.
///
/// Spelled out rather than read back from the stubs because the conditional is
/// built when the function is parsed, before any constant lookup is available.
/// It is part of PHP's stable ABI.
const PATHINFO_ALL: i64 = 15;

/// Patch `print_r()` to have a conditional return type keyed on `$return`.
///
/// The stubs declare `string|bool` (`string|true` from 8.4 on). php-src only
/// ever returns `true` when it printed, so the `false` half is impossible and
/// the `string` half only exists for `print_r($v, true)`.
fn patch_print_r(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$return",
        PhpType::parse("true"),
        PhpType::string(),
        PhpType::parse("true"),
    );
}

/// Patch `hrtime()` to have a conditional return type keyed on `$as_number`.
///
/// The stubs declare `int[]|int|float|false`, but the two shapes are decided
/// by the argument: the number form is an `int` (a `float` on 32-bit builds)
/// and the array form is the `[seconds, nanoseconds]` pair.
fn patch_hrtime(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$as_number",
        PhpType::parse("true"),
        PhpType::union(vec![PhpType::int(), PhpType::float()]),
        PhpType::parse("array{int, int}|false"),
    );
}

/// Patch `microtime()` to have a conditional return type keyed on `$as_float`.
///
/// The stubs carry `#[TypeContract(true: "float", false: "string")]` on the
/// parameter, which says exactly this, but the attribute is not read; the
/// declared `string|float` union is all that survives.
fn patch_microtime(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$as_float",
        PhpType::parse("true"),
        PhpType::float(),
        PhpType::string(),
    );
}

/// Patch `getenv()` to have a conditional return type keyed on its name
/// argument.
///
/// Only the no-argument form returns the whole environment; naming a variable
/// returns its value, or `false` when it is not set. The stubs declare the
/// union of both, so every `getenv('NAME')` carries an impossible array
/// branch.
///
/// The parameter is keyed by position rather than by name: the stubs declare
/// both the pre-7.1 `$varname` and the current `$name`, and which one is in
/// play depends on the configured PHP version.
fn patch_getenv(func: &mut FunctionInfo) {
    let Some(name_param) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    conditional_on(
        func,
        name_param.as_str(),
        PhpType::null(),
        PhpType::generic_array(PhpType::string(), PhpType::string()),
        PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
    );
}

/// Patch `mb_convert_encoding()` to have a conditional return type keyed on
/// its subject.
///
/// Like the replace family, the function answers in the shape it was handed,
/// but the stubs can only declare `array|string|false`. An array subject is
/// converted per element, so no error branch survives there.
fn patch_mb_convert_encoding(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$string",
        PhpType::array(),
        PhpType::generic_array(PhpType::named(atom("array-key")), PhpType::string()),
        PhpType::union(vec![PhpType::string(), PhpType::named(atom("false"))]),
    );
}

/// Patch `abs()` to have a conditional return type keyed on `$num`.
///
/// `abs()` returns the type it was given; the declared `int|float` union
/// leaves an `int` argument's result carrying a `float` branch that cannot
/// happen. An argument that is neither (a numeric string, a `mixed`) leaves
/// both branches, which is all the call can promise.
fn patch_abs(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$num",
        PhpType::int(),
        PhpType::int(),
        PhpType::float(),
    );
}

/// Patch `var_export()` to have a conditional return type keyed on
/// `$return`.
///
/// The stubs declare `?string` for both forms, so the rendered string a
/// `var_export($v, true)` is written for carries a `null` it cannot be, and
/// the printing form promises a string it never hands back.
fn patch_var_export(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$return",
        PhpType::parse("true"),
        PhpType::string(),
        PhpType::null(),
    );
}

/// Patch `mb_internal_encoding()` to have a conditional return type keyed on
/// `$encoding`.
///
/// The one function is both the getter and the setter: without an argument it
/// reports the current internal encoding, and with one it reports whether the
/// change took. The stubs declare `string|bool` for both.
fn patch_mb_internal_encoding(func: &mut FunctionInfo) {
    let Some(encoding) = func.parameters.first().map(|p| p.name) else {
        return;
    };
    conditional_on(
        func,
        encoding.as_str(),
        PhpType::null(),
        PhpType::string(),
        PhpType::bool(),
    );
}

/// Patch `version_compare()` to have a conditional return type keyed on
/// `$operator`.
///
/// Naming an operator asks a yes/no question and gets a `bool`; leaving it out
/// asks for the ordering and gets `-1`, `0` or `1`. The stubs declare the
/// union of both.
fn patch_version_compare(func: &mut FunctionInfo) {
    conditional_on(
        func,
        "$operator",
        PhpType::null(),
        PhpType::int(),
        PhpType::bool(),
    );
}

/// Patch a member of the `scanf` family to have a conditional return type
/// keyed on its variadic out-parameters.
///
/// `sscanf($s, $format)` collects the parsed values into an array and returns
/// it; passing by-reference targets instead writes into them and returns how
/// many were assigned. The stubs carry exactly this on the variadic
/// (`#[TypeContract(exists: …, notExists: …)]`) but the attribute is not read,
/// leaving the flat union both forms share.
///
/// `assigned` is the count branch (`fscanf` adds a `false` for a read
/// failure); `collected` is the array branch.
fn patch_scanf_family(func: &mut FunctionInfo, assigned: &str, collected: &str) {
    let Some(vars) = func
        .parameters
        .last()
        .filter(|p| p.is_variadic)
        .map(|p| p.name)
    else {
        return;
    };
    conditional_on(
        func,
        vars.as_str(),
        PhpType::null(),
        PhpType::parse(collected),
        PhpType::parse(assigned),
    );
}

/// Patch a member of the replace family to have a conditional return type
/// keyed on its subject argument.
///
/// `preg_replace`, `str_replace` and their relatives take a subject that may
/// be either a string or an array of strings, and return the same shape they
/// were given. The stubs can only declare the flat union (`array|string`, plus
/// `null` for the `preg_` family, which returns `null` on a PCRE error), so a
/// call with a string subject carries an array branch that cannot happen, and
/// vice versa.
///
/// `subject_param` names the parameter holding the subject (`$string` for
/// `substr_replace`, `$subject` for the rest) and `nullable_on_error` marks
/// the functions whose string branch keeps the `null` error result. An array
/// subject is answered per element, so no error branch survives there.
///
/// A subject whose type cannot be pinned down at the call site leaves the
/// declared union, which is all the call can promise.
fn patch_replace_family(func: &mut FunctionInfo, subject_param: &str, nullable_on_error: bool) {
    // Bail out if the stub does not have the parameter the conditional keys
    // on: without it the subject cannot be identified and the conditional
    // would be decided against an unrelated argument.
    if !func.parameters.iter().any(|p| p.name == subject_param) {
        return;
    }

    // The subject's keys carry over untouched, so a string-keyed subject
    // keeps its string keys — hence `array-key` rather than `int`.
    let replaced_array =
        PhpType::generic_array(PhpType::named(atom("array-key")), PhpType::string());
    let replaced_string = if nullable_on_error {
        PhpType::union(vec![PhpType::string(), PhpType::null()])
    } else {
        PhpType::string()
    };

    func.conditional_return = Some(PhpType::conditional(
        subject_param,
        false,
        PhpType::array(),
        replaced_array,
        replaced_string,
    ));
}

/// Override the pre-8.4 return type of `stream_bucket_make_writeable()`.
///
/// phpstorm-stubs resolve the return type to bare `object|null` for PHP
/// versions before 8.4 (the `StreamBucket` class was only introduced in
/// 8.4). Bare `object` inside a union is not recognised by the type
/// engine's universal-container fallback the way `object` or `?object`
/// alone are, so `$bucket->data` / `$bucket->datalen` become
/// unverifiable. PHPStan's function map overrides this same case to
/// `stdClass|null`, which the type engine already treats as accepting
/// arbitrary properties. The real PHP 8.4+ `StreamBucket|null` type is
/// left untouched.
fn patch_stream_bucket_make_writeable(func: &mut FunctionInfo) {
    if func.return_type.as_ref().is_some_and(is_pre_84_object_type) {
        func.return_type = Some(PhpType::parse("stdClass|null"));
    }
    if func
        .native_return_type
        .as_ref()
        .is_some_and(is_pre_84_object_type)
    {
        func.native_return_type = Some(PhpType::parse("stdClass|null"));
    }
}

/// Whether `ty` is the pre-8.4 `object|null` (or bare `object`) shape,
/// as opposed to the real `StreamBucket|null` type used from 8.4 on.
fn is_pre_84_object_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => name.eq_ignore_ascii_case("object"),
        TypeKind::Nullable(inner) => is_pre_84_object_type(inner),
        TypeKind::Union(members) => members.iter().any(is_pre_84_object_type),
        _ => false,
    }
}
