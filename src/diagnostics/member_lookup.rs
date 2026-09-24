//! Member lookups shared by the member diagnostics: whether a class
//! declares a member of the kind an access asks for, with what
//! visibility, and whether a magic handler answers for it instead.

use crate::types::{ClassInfo, Visibility};

/// Which kind of member a lookup turned out to be, so the message can
/// name it without re-deriving it from the access syntax.
#[derive(Clone, Copy)]
pub(super) enum MemberKind {
    Method,
    Property,
    StaticProperty,
    Constant,
}

impl MemberKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            MemberKind::Method => "method",
            MemberKind::Property => "property",
            MemberKind::StaticProperty => "static property",
            MemberKind::Constant => "constant",
        }
    }

    /// Spell the member the way PHP's own error message does.
    pub(super) fn qualify(self, owner: &str, member_name: &str) -> String {
        match self {
            MemberKind::Method => format!("{}::{}()", owner, member_name),
            MemberKind::Property => format!("{}::${}", owner, member_name),
            // Extraction strips the `$` from `Foo::$bar`, so it is put
            // back here rather than being taken from the member name.
            MemberKind::StaticProperty => format!("{}::${}", owner, member_name),
            MemberKind::Constant => format!("{}::{}", owner, member_name),
        }
    }
}

/// The visibility and kind `class` declares `member_name` with, matching
/// the member kind the access syntax asks for.
///
/// A method call looks among the methods; a static access checks the
/// constants first (which also hold enum cases) and then the static
/// properties; an instance access checks the properties.  Method names are
/// compared case-insensitively and property and constant names
/// case-sensitively, which is how PHP compares them.
pub(super) fn declared_member(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
) -> Option<(Visibility, MemberKind)> {
    if is_method_call {
        return class
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(member_name))
            .map(|m| (m.visibility, MemberKind::Method));
    }

    if is_static {
        if let Some(constant) = class.constants.iter().find(|c| c.name == member_name) {
            return Some((constant.visibility, MemberKind::Constant));
        }
        // A static property is written `Foo::$bar`, and the stored name
        // may or may not carry the `$`.
        let bare = member_name.strip_prefix('$');
        return class
            .properties
            .iter()
            .find(|p| p.is_static && (p.name == member_name || bare.is_some_and(|n| p.name == n)))
            .map(|p| (p.visibility, MemberKind::StaticProperty));
    }

    class
        .properties
        .iter()
        .find(|p| p.name == member_name)
        .map(|p| (p.visibility, MemberKind::Property))
}

/// Whether an instance property access is answered by an Eloquent
/// relation rather than a declared property.
///
/// `$model->orderProducts` flows through `__get()` → `isRelation()` →
/// `method_exists()`, all of which are case-insensitive, so a
/// differently-cased access like `$model->orderproducts` resolves the same
/// relationship at runtime.  The relation is synthesized rather than
/// declared, so it carries no visibility of its own and is always public
/// in practice.
fn is_relation_property(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
) -> bool {
    !is_static
        && !is_method_call
        && crate::virtual_members::laravel::class_has_relation_method_ci(class, member_name)
}

/// Check whether a member exists on the fully-resolved class.
pub(crate) fn member_exists(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
) -> bool {
    declared_member(class, member_name, is_static, is_method_call).is_some()
        || is_relation_property(class, member_name, is_static, is_method_call)
}

/// Check whether a member exists on the class *and* is public.
///
/// Used by the shortcut that runs before the inheritance merge, where a
/// non-public member cannot be judged: the class in hand may be missing
/// the magic handler that would answer for it and the ancestor that
/// really declares it.  Confirming a public member is safe there, since
/// nothing further up can make a public member unreachable.
pub(super) fn member_is_public(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
) -> bool {
    matches!(
        declared_member(class, member_name, is_static, is_method_call),
        Some((Visibility::Public, _))
    ) || is_relation_property(class, member_name, is_static, is_method_call)
}

/// Relaxed member check for docblock references (`@see Class::member`).
///
/// PHPDoc `@see` uses `::` notation for all members (instance properties,
/// instance methods, static properties, constants), so we check every
/// member kind regardless of `is_static` or `is_method_call`.
pub(super) fn member_exists_relaxed(class: &ClassInfo, member_name: &str) -> bool {
    class
        .methods
        .iter()
        .any(|m| m.name.eq_ignore_ascii_case(member_name))
        || class.properties.iter().any(|p| p.name == member_name)
        || class.constants.iter().any(|c| c.name == member_name)
}

/// Check whether the class has a magic method that would handle the
/// member access at runtime, making the "unknown member" diagnostic
/// a false positive.
///
/// For property access, `__get` only suppresses the diagnostic when
/// the class has no `@property` annotations.  When `@property` tags
/// exist, they define the expected property surface and unknown
/// properties should be flagged (matching PHPStan's behaviour with
/// `reportMagicProperties: true`).
pub(super) fn has_magic_method_for_access(
    class: &ClassInfo,
    is_static: bool,
    is_method_call: bool,
    report_magic_properties: bool,
) -> bool {
    if is_method_call {
        let magic = if is_static { "__callStatic" } else { "__call" };
        return class
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(magic));
    }

    if !is_static {
        // Instance property access — `__get` handles arbitrary property
        // names.  When `report_magic_properties` is enabled and any
        // virtual member provider has added properties to the class
        // (@property docblock tags, Laravel Eloquent column inference,
        // etc.), do not suppress — let normal member checking flag
        // unknowns.  When disabled (the default), `__get` always
        // suppresses.
        let has_get = class
            .methods
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case("__get"));
        if has_get {
            if report_magic_properties {
                let has_virtual_properties = class.properties.iter().any(|p| p.is_virtual);
                return !has_virtual_properties;
            }
            return true;
        }
    }

    false
}

/// Name the class for a message, preferring the FQN.
pub(super) fn display_class_name(class: &ClassInfo) -> String {
    if class.name.starts_with("__anonymous@") {
        return "anonymous class".to_string();
    }
    class.fqn().to_string()
}
