//! The `$args` docblock Mockery's mock factories share.
//!
//! `Mockery::mock()`, `spy()`, `Mockery\Container::mock()` and the global
//! `mock()` / `spy()` / `namedMock()` helpers take `...$args` tagged
//! `@param array<class-string<TMock>|TMock|Closure(…)|array<TMock>> $args`.
//! The `array<TMock>` alternative is wrong: an array argument is the mocked
//! class's constructor arguments or a map of expectations, so
//! `Mockery::mock(Client::class, ['api_key'])` is a mismatch as written.
//! The alternative becomes a plain `array`, as `Mockery::namedMock()`
//! already spells it (`array<mixed>`), leaving the alternatives that bind
//! `TMock` untouched.

use crate::atom::Atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ParameterInfo;

/// The methods of `fqn` that take Mockery's `$args`.
pub(super) fn mock_methods_of(fqn: &str) -> Option<&'static [&'static str]> {
    match fqn {
        "Mockery" => Some(&["mock", "spy", "namedMock"]),
        "Mockery\\Container" => Some(&["mock"]),
        _ => None,
    }
}

/// Whether a global function named `name` may be one of Mockery's helpers.
pub(super) fn is_mock_helper(name: &str) -> bool {
    matches!(name, "mock" | "spy" | "namedMock")
}

/// Whether `parameters` end in Mockery's `...$args` with an array
/// alternative typed by one of `templates`.
pub(super) fn declares_mock_args(parameters: &[ParameterInfo], templates: &[Atom]) -> bool {
    parameters
        .last()
        .filter(|p| p.is_variadic && p.name == "$args" && p.native_type_hint.is_none())
        .and_then(|p| p.type_hint.as_ref())
        .and_then(|hint| widened(hint, templates))
        .is_some()
}

/// Replace the `array<TMock>` alternative of `...$args` with `array`.
pub(super) fn widen_mock_args(parameters: &mut [ParameterInfo], templates: &[Atom]) {
    let Some(param) = parameters.last_mut() else {
        return;
    };
    if let Some(hint) = param.type_hint.as_ref().and_then(|h| widened(h, templates)) {
        param.type_hint = Some(hint);
    }
}

/// `array<…|array<T>|…>` with `array<T>` replaced by `array`, or `None`
/// when `hint` has no such alternative.
fn widened(hint: &PhpType, templates: &[Atom]) -> Option<PhpType> {
    let TypeKind::Generic(outer) = hint.kind() else {
        return None;
    };
    let [element] = &outer.args[..] else {
        return None;
    };
    if !outer.name.eq_ignore_ascii_case("array") {
        return None;
    }
    let TypeKind::Union(members) = element.kind() else {
        return None;
    };
    let is_template_array = |member: &PhpType| match member.kind() {
        TypeKind::Generic(g) if g.name.eq_ignore_ascii_case("array") => {
            matches!(&g.args[..], [arg] if templates.iter().any(|t| arg.is_named(t)))
        }
        _ => false,
    };
    if !members.iter().any(is_template_array) {
        return None;
    }
    let members = members
        .iter()
        .map(|m| {
            if is_template_array(m) {
                PhpType::array()
            } else {
                m.clone()
            }
        })
        .collect();
    Some(PhpType::generic(
        outer.name.as_str(),
        vec![PhpType::union(members)],
    ))
}
