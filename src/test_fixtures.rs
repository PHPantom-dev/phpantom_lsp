//! Shared test fixture helpers for unit tests.
//!
//! These helpers construct minimal instances of the core data types
//! (`ClassInfo`, `MethodInfo`, `PropertyInfo`, `ConstantInfo`,
//! `ParameterInfo`) with sensible defaults.  Every `#[cfg(test)]`
//! module that previously copy-pasted its own `make_class()` /
//! `make_method()` / etc. should import from here instead.

use std::sync::Arc;

use crate::Backend;
use crate::php_type::PhpType;
use crate::types::{
    ClassInfo, ClassLikeKind, ConstantInfo, MethodInfo, ParameterInfo, PropertyInfo, Visibility,
};

/// Create a minimal [`Backend`] for unit tests.
///
/// This is the in-crate equivalent of `create_test_backend()` in the
/// integration test helpers (`tests/common/mod.rs`).
pub fn make_backend() -> Backend {
    Backend::new_test()
}

// Note: `Visibility` is still needed by `make_class` and `make_constant`.

/// Create a minimal `ClassInfo` with the given name.
///
/// All collection fields are empty, all flags are `false`, and the kind
/// is [`ClassLikeKind::Class`].
pub fn make_class(name: &str) -> ClassInfo {
    ClassInfo {
        kind: ClassLikeKind::Class,
        name: crate::atom::atom(name),
        ..Default::default()
    }
}

/// Create a `MethodInfo` with the given name and return type.
///
/// The method is public, non-static, non-deprecated, with no parameters
/// and no template params.  Delegates to
/// [`MethodInfo::virtual_method`].
pub fn make_method(name: &str, return_type: Option<&str>) -> MethodInfo {
    MethodInfo::virtual_method(name, return_type)
}

/// Create a `MethodInfo` with the given name, return type, and parameters.
pub fn make_method_with_params(
    name: &str,
    return_type: Option<&str>,
    params: Vec<ParameterInfo>,
) -> MethodInfo {
    MethodInfo {
        parameters: params.into(),
        ..MethodInfo::virtual_method(name, return_type)
    }
}

/// Create a `PropertyInfo` with the given name and type hint.
///
/// The property is public, non-static, and non-deprecated.  Delegates
/// to [`PropertyInfo::virtual_property`].
pub fn make_property(name: &str, type_hint: Option<&str>) -> PropertyInfo {
    PropertyInfo::virtual_property(name, type_hint)
}

/// Create a `ConstantInfo` with the given name.
///
/// The constant is public, non-deprecated, and has no type hint.
pub fn make_constant(name: &str) -> ConstantInfo {
    ConstantInfo {
        name: crate::atom::atom(name),
        name_offset: 0,
        type_hint: None,
        visibility: Visibility::Public,
        deprecation_message: None,
        deprecated_replacement: None,
        see_refs: Vec::new(),
        description: None,
        is_enum_case: false,
        enum_value: None,
        value: None,
        is_virtual: true,
    }
}

/// Create a `ParameterInfo` with the given name, type hint, and
/// required flag.
///
/// The parameter is non-variadic and non-reference.
pub fn make_param(name: &str, type_hint: Option<&str>, is_required: bool) -> ParameterInfo {
    ParameterInfo {
        name: crate::atom::atom(name),
        is_required,
        type_hint: type_hint.map(PhpType::parse),
        native_type_hint: type_hint.map(PhpType::parse),
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
        param_out_type: None,
        callable_invocation: None,
    }
}

/// A class loader that always returns `None`.
///
/// Useful for tests that don't need cross-class resolution.
pub fn no_loader(_name: crate::class_lookup::Fqn<'_>) -> Option<Arc<ClassInfo>> {
    None
}

/// [`no_loader`] for names as written in source.
pub fn no_source_loader(_name: &str) -> Option<Arc<ClassInfo>> {
    None
}

/// A loader for names as written that reads every name as fully qualified
/// through `loader`, for tests whose names are all written that way.
///
/// Leaked, so a helper can hand it out inside the context it builds.
pub fn fqn_source_loader<'a>(
    loader: &'a crate::class_lookup::ResolvedClassLoader<'a>,
) -> &'a crate::class_lookup::SourceClassLoader<'a> {
    Box::leak(Box::new(move |name: &str| {
        loader(crate::class_lookup::Fqn::new(name))
    }))
}
