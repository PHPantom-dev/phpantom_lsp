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
        methods: Default::default(),
        method_index: Default::default(),
        indexed_method_count: 0,
        properties: Default::default(),
        constants: Default::default(),
        start_offset: 0,
        end_offset: 0,
        keyword_offset: 0,
        decl_start_offset: 0,
        parent_class: None,
        interfaces: Vec::new(),
        used_traits: Vec::new(),
        mixins: Vec::new(),
        mixin_generics: Vec::new(),
        require_extends: None,
        require_implements: Vec::new(),
        is_final: false,
        is_abstract: false,
        deprecation_message: None,
        deprecated_replacement: None,
        links: Vec::new(),
        see_refs: Vec::new(),
        template_params: Vec::new(),
        template_param_bounds: Default::default(),
        template_param_defaults: Default::default(),
        extends_generics: Vec::new(),
        implements_generics: Vec::new(),
        use_generics: Vec::new(),
        type_aliases: Default::default(),
        trait_precedences: Vec::new(),
        trait_aliases: Vec::new(),
        class_docblock: None,
        file_namespace: None,
        backed_type: None,
        attribute_targets: 0,
        laravel: None,
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
        parameters: params,
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
    }
}

/// A class loader that always returns `None`.
///
/// Useful for tests that don't need cross-class resolution.
pub fn no_loader(_name: &str) -> Option<Arc<ClassInfo>> {
    None
}
