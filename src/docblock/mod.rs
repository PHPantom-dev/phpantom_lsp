//! PHPDoc block parsing.
//!
//! This module extracts type information from PHPDoc comments (`/** ... */`).
//! It is split into focused submodules:
//!
//! # Submodules
//!
//! - `tags`: Core PHPDoc tag extraction (`@return`, `@var`, `@param`,
//!   `@mixin`, `@deprecated`, `@phpstan-assert`, docblock text retrieval,
//!   and type override logic).
//! - `templates`: Template, generics, and type alias tag extraction
//!   (`@template`, `@extends`, `@implements`, `@use`, `@phpstan-type`,
//!   `@phpstan-import-type`, and `class-string<T>` conditional synthesis).
//! - `virtual_members`: Virtual member tag extraction (`@property`,
//!   `@property-read`, `@property-write`, `@method`).
//! - `conditional`: PHPStan conditional return type parsing.
//! - `type_strings`: Foundational type string manipulation (constants,
//!   splitting, cleaning, stripping, scalar checks, self/static replacement)
//! - `shapes`: Array shape and object shape parsing

mod conditional;
pub(crate) mod parser;
mod tags;
pub(crate) mod templates;
mod virtual_members;

// Type sub-modules.
pub(crate) mod shapes;
pub(crate) mod type_strings;

// ─── Re-exports ─────────────────────────────────────────────────────────────
//
// Everything below was previously a public or crate-visible item in the
// single-file `docblock.rs`.  Re-exporting here keeps all existing call
// sites (`use crate::docblock;` and `use phpantom_lsp::docblock::*;`)
// working without modification.

// Parsed docblock representation
pub use parser::{DocblockInfo, parse_docblock_for_tags};

// Core tags
pub(crate) use tags::is_compatible_refinement_typed;
pub use tags::{
    extract_all_param_tags, extract_all_param_tags_from_info, extract_deprecation_message,
    extract_deprecation_message_from_info, extract_deprecation_with_see,
    extract_deprecation_with_see_from_info, extract_if_this_is_type, extract_link_urls,
    extract_link_urls_from_info, extract_mixin_tags, extract_mixin_tags_from_info,
    extract_param_closure_this, extract_param_closure_this_from_info, extract_param_description,
    extract_param_description_from_info, extract_param_raw_type, extract_param_raw_type_from_info,
    extract_param_types_positional_from_info, extract_removed_version, extract_require_extends,
    extract_require_extends_from_info, extract_return_description,
    extract_return_description_from_info, extract_return_type, extract_return_type_from_info,
    extract_see_references, extract_see_references_from_info, extract_throws_tags,
    extract_throws_tags_from_info, extract_type_assertions, extract_type_assertions_from_info,
    extract_var_type, extract_var_type_from_info, extract_var_type_with_name,
    extract_var_type_with_name_from_info, find_enclosing_return_type, find_inline_var_docblock,
    find_iterable_raw_type_in_source, find_var_raw_type_in_source, get_docblock_info_for_node,
    get_docblock_text_for_node, has_deprecated_tag, has_deprecated_tag_from_info,
    resolve_effective_type_typed, sanitise_and_parse_docblock_type, should_override_type_typed,
};

// Template / generics / type alias tags
pub use templates::{
    extract_generics_tag, extract_generics_tag_from_info, extract_template_param_bindings,
    extract_template_param_bindings_from_info, extract_template_params,
    extract_template_params_from_info, extract_template_params_full,
    extract_template_params_full_from_info, extract_template_params_with_bounds,
    extract_template_params_with_bounds_from_info, extract_type_aliases,
    extract_type_aliases_from_info, synthesize_template_conditional,
    synthesize_template_conditional_from_info,
};

// Virtual member tags
pub use virtual_members::{extract_method_tags, extract_property_tags};

// Conditional return types
pub use conditional::{extract_conditional_return_type, extract_conditional_return_type_from_info};

// Type utilities
pub use shapes::{
    extract_array_shape_value_type_typed, extract_object_shape_property_type_typed,
    is_object_shape_typed, parse_array_shape_typed, parse_object_shape_typed,
};
