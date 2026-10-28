//! PHPDoc block parsing.
//!
//! This module extracts type information from PHPDoc comments (`/** ... */`).
//! It is split into focused submodules:
//!
//! # Submodules
//!
//! - `parser`: Parsing adapter bridging raw docblock text to the
//!   structured `mago-phpdoc-syntax` tag representation (`DocblockInfo`,
//!   `TagInfo`).
//! - `tag_kind`: Vendor-agnostic tag classification (`TagKind`), folding
//!   `@psalm-`/`@phpstan-` prefixed tags together with their bare form.
//! - `tags`: Core PHPDoc tag extraction (`@return`, `@var`, `@param`,
//!   `@mixin`, `@deprecated`, `@phpstan-assert`, docblock text retrieval).
//! - `type_override`: Choosing between a docblock type and a native type
//!   hint.
//! - `lines`: Reading one docblock line as source text.
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
mod lines;
pub(crate) mod parser;
mod tag_kind;
mod tags;
pub(crate) mod templates;
mod type_override;
mod virtual_members;

// Type sub-modules.
pub(crate) mod shapes;
pub(crate) mod type_strings;

// ─── Re-exports ─────────────────────────────────────────────────────────────
//
// Flattens the submodule tree so call sites can `use crate::docblock;` /
// `use phpantom_lsp::docblock::*;` without knowing which submodule an item
// actually lives in.

// Parsed docblock representation
pub use parser::{DocblockInfo, TagInfo, parse_docblock_for_tags};
pub(crate) use tag_kind::tag_kind;
pub use tag_kind::{TagKind, TagVendor};

// Core tags
pub(crate) use tags::merge_param_docblock_into_parameters;
pub use tags::{
    declares_impure, declares_pure, extract_all_param_tags, extract_all_param_tags_from_info,
    extract_deprecation_message, extract_deprecation_message_from_info,
    extract_deprecation_with_see, extract_deprecation_with_see_from_info,
    extract_doc_readonly_from_info, extract_if_this_is_type, extract_if_this_is_type_from_info,
    extract_link_urls, extract_link_urls_from_info, extract_mixin_tags,
    extract_mixin_tags_from_info, extract_param_closure_this, extract_param_closure_this_from_info,
    extract_param_description, extract_param_description_from_info, extract_param_raw_type,
    extract_param_raw_type_from_info, extract_param_types_positional_from_info,
    extract_removed_version, extract_require_extends, extract_require_extends_from_info,
    extract_require_implements, extract_require_implements_from_info, extract_return_description,
    extract_return_description_from_info, extract_return_type, extract_return_type_from_info,
    extract_see_references, extract_see_references_from_info, extract_see_refs_from_info,
    extract_self_out_type, extract_self_out_type_from_info, extract_throws_tags,
    extract_throws_tags_from_info, extract_type_assertions, extract_type_assertions_from_info,
    extract_var_type, extract_var_type_from_info, extract_var_type_with_name,
    extract_var_type_with_name_from_info, get_docblock_info_for_node, get_docblock_text_for_node,
    has_deprecated_tag, has_deprecated_tag_from_info, sanitise_and_parse_docblock_type,
};

// Docblock vs. native type selection
pub(crate) use type_override::is_compatible_refinement_typed;
pub use type_override::{
    resolve_effective_type_typed, resolve_effective_type_with_template_bounds,
    should_override_type_typed,
};

// Docblock lines as source text
pub(crate) use lines::{find_var_tag, strip_docblock_line_delimiters};

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
pub use virtual_members::{
    extract_method_tags, extract_method_tags_from_info, extract_property_tags,
    extract_property_tags_from_info,
};

// Conditional return types
pub use conditional::{extract_conditional_return_type, extract_conditional_return_type_from_info};

// Type utilities
pub use shapes::{
    extract_array_shape_value_type_typed, extract_object_shape_property_type_typed,
    is_object_shape_typed, parse_array_shape_typed, parse_object_shape_typed,
};
