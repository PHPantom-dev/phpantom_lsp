//! Eloquent cast type resolution.
//!
//! This module maps Eloquent `$casts` type strings (e.g. `datetime`,
//! `boolean`, `App\Casts\MoneyCast`) to their corresponding PHP types
//! for virtual property synthesis.  It handles built-in cast strings,
//! `decimal:N` and `datetime:format` variants, Laravel's class-based
//! casts (`AsEnumCollection::of(Status::class)` and the rest of the
//! `As*` family), custom cast classes (via
//! `@implements CastsAttributes<TGet, TSet>` or, failing that, `get()`
//! return type inspection), enum casts, and `Castable` implementations.

use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ClassLikeKind};
use crate::util::short_name;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

/// The short name of the `CastsAttributes` interface, used to look up
/// `@implements` generic arguments on custom cast classes.
const CASTS_ATTRIBUTES_SHORT: &str = "CastsAttributes";

/// The fully-qualified name of the `CastsAttributes` interface.
const CASTS_ATTRIBUTES_FQN: &str = "Illuminate\\Contracts\\Database\\Eloquent\\CastsAttributes";

/// Maps Eloquent cast type strings to their corresponding PHP types.
///
/// When a model declares `protected $casts = ['col' => 'datetime']`, the
/// column is treated as `\Carbon\Carbon` in completions.  This table
/// covers all built-in Laravel cast types.
static CAST_TYPE_MAP: LazyLock<HashMap<&'static str, PhpType>> = LazyLock::new(|| {
    HashMap::from([
        (
            "datetime",
            PhpType::named(atom(super::CONFIGURED_DATE_CLASS_FQN)),
        ),
        (
            "date",
            PhpType::named(atom(super::CONFIGURED_DATE_CLASS_FQN)),
        ),
        ("timestamp", PhpType::int()),
        (
            "immutable_datetime",
            PhpType::named(atom("Carbon\\CarbonImmutable")),
        ),
        (
            "immutable_date",
            PhpType::named(atom("Carbon\\CarbonImmutable")),
        ),
        ("boolean", PhpType::bool()),
        ("bool", PhpType::bool()),
        ("integer", PhpType::int()),
        ("int", PhpType::int()),
        ("float", PhpType::float()),
        ("double", PhpType::float()),
        ("real", PhpType::float()),
        ("string", PhpType::string()),
        ("array", PhpType::array()),
        ("json", PhpType::array()),
        ("object", PhpType::object()),
        (
            "collection",
            PhpType::named(atom("Illuminate\\Support\\Collection")),
        ),
        ("encrypted", PhpType::string()),
        ("encrypted:array", PhpType::array()),
        (
            "encrypted:collection",
            PhpType::named(atom("Illuminate\\Support\\Collection")),
        ),
        ("encrypted:object", PhpType::object()),
        ("hashed", PhpType::string()),
    ])
});

/// Laravel configured date type used by date/datetime casts.
fn carbon_type() -> PhpType {
    PhpType::named(atom(super::CONFIGURED_DATE_CLASS_FQN))
}

/// Pre-built `PhpType` for `\Carbon\CarbonImmutable`, used by immutable date casts.
fn carbon_immutable_type() -> PhpType {
    PhpType::named(atom("Carbon\\CarbonImmutable"))
}

/// The fully-qualified name of the `Castable` contract.
const CASTABLE_FQN: &str = "Illuminate\\Contracts\\Database\\Eloquent\\Castable";

/// Map an Eloquent cast type string to a PHP type.
///
/// Handles built-in cast strings (`datetime`, `boolean`, `array`, etc.),
/// `decimal:N` variants (e.g. `decimal:2` → `float`), Laravel's `As*`
/// class casts (the property type is the value the cast returns, with
/// generics filled from `of()` / `using()`), custom cast classes, enum
/// classes (the property type is the enum itself), and `Castable`
/// implementations (the property type is the class itself).
///
/// For custom cast classes, the first generic argument from an
/// `@implements CastsAttributes<TGet, TSet>` annotation takes priority,
/// since it is the developer's explicit contract.  When no such
/// annotation is present, the resolver falls back to the `get()`
/// method's return type.
///
/// Class-based cast types may carry a `:argument` suffix (e.g.
/// `Address::class.':nullable'`).  The suffix is stripped before
/// resolving the class.
pub(super) fn cast_type_to_php_type(
    cast_type: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> PhpType {
    // 1. Check the built-in mapping table.
    let lower = cast_type.to_lowercase();
    if let Some(php_type) = CAST_TYPE_MAP.get(lower.as_str()) {
        return php_type.clone();
    }

    // 2. Handle `decimal:N` variants (e.g. `decimal:2`, `decimal:8`).
    if lower.starts_with("decimal:") || lower == "decimal" {
        return PhpType::float();
    }

    // 3. Handle `datetime:format` variants (e.g. `datetime:Y-m-d`).
    if lower.starts_with("datetime:") {
        return carbon_type();
    }

    // 4. Handle `date:format` variants.
    if lower.starts_with("date:") {
        return carbon_type();
    }

    // 5. Handle `immutable_datetime:format` variants.
    if lower.starts_with("immutable_datetime:") {
        return carbon_immutable_type();
    }

    // 6. Handle `immutable_date:format` variants.
    if lower.starts_with("immutable_date:") {
        return carbon_immutable_type();
    }

    // 7. Laravel's own class-based casts (`AsEnumCollection::of(Status::class)`,
    //    `AsCollection::using(...)`, `AsArrayObject::class`, …) return a
    //    collection, array object, or scalar — not an instance of the cast
    //    class.  They implement `Castable`, so this has to run before the
    //    generic Castable rule below, which would otherwise type the
    //    property as the cast class itself.
    if let Some(php_type) = framework_cast_type(cast_type) {
        return php_type;
    }

    // 8. Assume it's a class-based cast.  Strip any `:argument` suffix
    //    (e.g. `App\Casts\Address:nullable` → `App\Casts\Address`).
    //    A stored method call (`Foo::of:Bar`) separates the class at `::`,
    //    and `split(':')` would stop at the first colon of that separator.
    let class_name = cast_class_name(cast_type);

    if let Some(cast_class) = class_loader(class_name) {
        // 8a. Enums — the property type is the enum itself.
        if cast_class.kind == ClassLikeKind::Enum {
            return PhpType::named(atom(class_name));
        }

        // 8b. Castable implementations — the property type is the
        //     class itself.  Castable classes declare `castUsing()`
        //     which returns a CastsAttributes instance, but the
        //     developer-facing type is the Castable class.
        if is_castable(&cast_class) {
            return PhpType::named(atom(class_name));
        }

        // 8c. `@implements CastsAttributes<TGet, TSet>` — the canonical
        //     type declaration.  The class-level generic annotation is
        //     the strongest signal because it is the developer's
        //     explicit contract.  The `get()` method's return type is
        //     an implementation detail that may be `mixed`, less
        //     specific, or missing entirely.
        if let Some(tget) = extract_tget_from_implements_generics(&cast_class) {
            return tget;
        }

        // 8d. Fallback: inspect the `get()` method's return type.
        //     When no `@implements` generics are declared, the concrete
        //     return type on `get()` is the next best signal.  Skip
        //     `mixed` — it carries no useful type information and is
        //     the default native hint on the interface method.
        if let Some(get_method) = cast_class.get_method("get")
            && let Some(ref rt) = get_method.return_type
            && !rt.is_mixed()
        {
            return rt.clone();
        }
    }

    // 9. Fallback: unknown cast type.
    PhpType::mixed()
}

/// The class a cast type string names, without its argument suffix.
///
/// `App\Casts\Address:nullable` is `App\Casts\Address`. A method call stored
/// as `AsEnumCollection::of:Status` is `AsEnumCollection`: the `::` is the
/// method separator, not an argument separator, so the first `:` of `::`
/// must not end the class name.
fn cast_class_name(cast_type: &str) -> &str {
    let (class, _, _) = parse_cast_reference(cast_type);
    class
}

const FRAMEWORK_CAST_NS: &str = "Illuminate\\Database\\Eloquent\\Casts\\";
const SUPPORT_COLLECTION_FQN: &str = "Illuminate\\Support\\Collection";
const ARRAY_OBJECT_FQN: &str = "Illuminate\\Database\\Eloquent\\Casts\\ArrayObject";

/// Map one of Laravel's `As*` cast classes to the value it returns.
///
/// Two spellings reach here. A method call in source is stored with the
/// method kept (`AsEnumCollection::of:Status`, `AsCollection::using:Custom,Item`)
/// so an alias of the cast class still says which argument is the enum and
/// which is the collection. The string Eloquent stores at runtime
/// (`AsEnumCollection:Status`, `AsCollection:,Item`, `AsCollection:Custom,`)
/// has no method, and the argument positions are the ones `of()` / `using()`
/// implode into.
///
/// Returns `None` for anything that is not one of those classes, including a
/// user class that happens to share the short name.
fn framework_cast_type(cast_type: &str) -> Option<PhpType> {
    let (class, method, args) = parse_cast_reference(cast_type);
    let short = framework_cast_short(class)?;
    Some(match short {
        "AsArrayObject" | "AsEncryptedArrayObject" => array_object_of(PhpType::mixed()),
        "AsCollection" | "AsEncryptedCollection" => as_collection_type(method, &args),
        "AsEnumCollection" => collection_of(enum_arg(method, &args)),
        "AsEnumArrayObject" => array_object_of(enum_arg(method, &args)),
        "AsStringable" => PhpType::named(atom("Illuminate\\Support\\Stringable")),
        "AsFluent" => PhpType::named(atom("Illuminate\\Support\\Fluent")),
        "AsHtmlString" => PhpType::named(atom("Illuminate\\Support\\HtmlString")),
        "AsUri" => PhpType::named(atom("Illuminate\\Support\\Uri")),
        "AsBinary" => PhpType::string(),
        "AsVector" => PhpType::generic("array", vec![PhpType::int(), PhpType::float()]),
        _ => return None,
    })
}

/// The short name of a framework cast class, or `None` when `class` is not one.
///
/// A bare short name matches too, so a cast string that has not been resolved
/// to an FQN yet (a unit test, a file with no namespace) still maps. A class
/// in any other namespace does not, even when its short name collides.
fn framework_cast_short(class: &str) -> Option<&str> {
    let class = class.strip_prefix('\\').unwrap_or(class);
    let short = short_name(class);
    let known = matches!(
        short,
        "AsArrayObject"
            | "AsEncryptedArrayObject"
            | "AsCollection"
            | "AsEncryptedCollection"
            | "AsEnumCollection"
            | "AsEnumArrayObject"
            | "AsStringable"
            | "AsFluent"
            | "AsHtmlString"
            | "AsUri"
            | "AsBinary"
            | "AsVector"
    )
    .then_some(short)?;
    (class == known || class.strip_prefix(FRAMEWORK_CAST_NS) == Some(known)).then_some(known)
}

/// `AsCollection` / `AsEncryptedCollection`, from either spelling.
///
/// `of(Item::class)` maps each JSON element into `Item`. `using(Custom::class)`
/// instantiates `Custom` instead of the base collection, and the two-argument
/// form does both. With no argument the value is a `Collection` of `mixed`.
fn as_collection_type(method: Option<&str>, args: &[&str]) -> PhpType {
    match method {
        Some("of") => collection_of(named_or_mixed(args.first().copied().unwrap_or(""))),
        Some("using") => collection_type(
            args.first().copied().unwrap_or(""),
            args.get(1).copied().unwrap_or(""),
        ),
        Some(_) => collection_of(PhpType::mixed()),
        None => collection_type(
            args.first().copied().unwrap_or(""),
            args.get(1).copied().unwrap_or(""),
        ),
    }
}

/// A collection class plus the class each item is mapped into.
///
/// An empty collection class is the base `Collection`. An empty item class
/// leaves a custom collection as itself, so its own template defaults apply.
fn collection_type(collection: &str, item: &str) -> PhpType {
    if item.is_empty() {
        if collection.is_empty() {
            collection_of(PhpType::mixed())
        } else {
            PhpType::named(atom(collection))
        }
    } else if collection.is_empty() {
        collection_of(PhpType::named(atom(item)))
    } else {
        keyed_generic(collection, PhpType::named(atom(item)))
    }
}

/// The enum `AsEnumCollection::of()` / `AsEnumArrayObject::of()` names.
///
/// Both spellings put it first: the method form as the only argument, the
/// runtime string as the sole `:` argument.
fn enum_arg(method: Option<&str>, args: &[&str]) -> PhpType {
    let name = match method {
        Some("of") | None => args.first().copied().unwrap_or(""),
        Some(_) => "",
    };
    named_or_mixed(name)
}

fn named_or_mixed(name: &str) -> PhpType {
    if name.is_empty() {
        PhpType::mixed()
    } else {
        PhpType::named(atom(name))
    }
}

fn collection_of(value: PhpType) -> PhpType {
    keyed_generic(SUPPORT_COLLECTION_FQN, value)
}

fn array_object_of(value: PhpType) -> PhpType {
    keyed_generic(ARRAY_OBJECT_FQN, value)
}

fn keyed_generic(class: &str, value: PhpType) -> PhpType {
    PhpType::generic(class, vec![PhpType::named(atom("array-key")), value])
}

/// A cast type string split into the class, an optional method, and arguments.
///
/// `AsEnumCollection::of:Status` is `(class, Some("of"), ["Status"])`.
/// `AsCollection:,Item` is `(class, None, ["", "Item"])`. `boolean` is
/// `(class, None, [])`.
fn parse_cast_reference(cast_type: &str) -> (&str, Option<&str>, Vec<&str>) {
    if let Some(sep) = cast_type.find("::") {
        let class = &cast_type[..sep];
        let rest = &cast_type[sep + 2..];
        if let Some(colon) = rest.find(':') {
            return (
                class,
                Some(&rest[..colon]),
                split_cast_args(&rest[colon + 1..]),
            );
        }
        return (class, Some(rest), Vec::new());
    }
    if let Some(colon) = cast_type.find(':') {
        return (
            &cast_type[..colon],
            None,
            split_cast_args(&cast_type[colon + 1..]),
        );
    }
    (cast_type, None, Vec::new())
}

fn split_cast_args(args: &str) -> Vec<&str> {
    if args.is_empty() {
        Vec::new()
    } else {
        args.split(',').map(str::trim).collect()
    }
}

/// Join a cast reference back into the string [`parse_cast_reference`] reads.
pub(in crate::virtual_members::laravel) fn format_cast_reference(
    class: &str,
    method: Option<&str>,
    args: &[String],
) -> String {
    match method {
        Some(method) if args.is_empty() => format!("{class}::{method}"),
        Some(method) => format!("{class}::{method}:{}", args.join(",")),
        None if args.is_empty() => class.to_string(),
        None => format!("{class}:{}", args.join(",")),
    }
}

/// Resolve the class names inside a cast type string.
///
/// `resolve` turns a written class name into its FQN the way the file's
/// imports would. Arguments that are not class names (`sha256`, `Y-m-d`,
/// `uuid`) stay as written, and a name that already contains `\` is not
/// passed through `resolve` — that would prepend the file's namespace onto
/// an already-qualified name.
pub(crate) fn qualify_cast_type(cast_type: &str, resolve: impl Fn(&str) -> String) -> String {
    let (class, method, args) = parse_cast_reference(cast_type);
    let resolved_class = qualify_cast_class(class, &resolve);
    let resolved_args: Vec<String> = args
        .iter()
        .map(|arg| qualify_cast_arg(arg, &resolve))
        .collect();
    format_cast_reference(&resolved_class, method, &resolved_args)
}

fn qualify_cast_class(name: &str, resolve: &impl Fn(&str) -> String) -> String {
    if name.contains('\\') {
        name.strip_prefix('\\').unwrap_or(name).to_string()
    } else if name.chars().any(|c| c.is_ascii_uppercase()) {
        resolve(name)
    } else {
        name.to_string()
    }
}

fn qualify_cast_arg(arg: &str, resolve: &impl Fn(&str) -> String) -> String {
    if !is_cast_class_arg(arg) {
        return arg.to_string();
    }
    if arg.contains('\\') {
        arg.strip_prefix('\\').unwrap_or(arg).to_string()
    } else {
        resolve(arg)
    }
}

/// Whether a cast argument is a class name rather than a format or flag.
///
/// `ServerStatus` and `App\Enums\Status` are. `uuid`, `sha256`, `nullable`,
/// `2`, and `Y-m-d` are not: a class argument starts with an uppercase letter
/// and contains only characters legal in a class name.
fn is_cast_class_arg(arg: &str) -> bool {
    let name = arg.strip_prefix('\\').unwrap_or(arg);
    !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\\')
        && !name.ends_with('\\')
}

/// Extract the `TGet` type from a cast class's `@implements CastsAttributes<TGet, TSet>`.
///
/// Returns the first generic argument if the class declares an
/// `@implements` annotation for `CastsAttributes` (matched by short
/// name or FQN, with or without leading backslash).
fn extract_tget_from_implements_generics(class: &ClassInfo) -> Option<PhpType> {
    for (name, args) in &class.implements_generics {
        if (name == CASTS_ATTRIBUTES_FQN
            || name == CASTS_ATTRIBUTES_SHORT
            || short_name(name) == CASTS_ATTRIBUTES_SHORT)
            && let Some(tget) = args.first()
        {
            // Skip empty/blank type arguments (e.g. from malformed docblocks).
            if matches!(tget.kind(), TypeKind::Named(s) if s.is_empty())
                || matches!(tget.kind(), TypeKind::Raw(s) if s.is_empty())
            {
                continue;
            }
            return Some(tget.clone());
        }
    }
    None
}

/// Check whether a class implements the `Castable` contract.
///
/// Looks for `Illuminate\Contracts\Database\Eloquent\Castable` in the
/// class's `interfaces` list (with or without leading backslash, and
/// also matches the short name `Castable`).
fn is_castable(class: &ClassInfo) -> bool {
    class
        .interfaces
        .iter()
        .any(|iface| iface == CASTABLE_FQN || iface == "Castable")
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "casts_tests.rs"]
mod tests;
