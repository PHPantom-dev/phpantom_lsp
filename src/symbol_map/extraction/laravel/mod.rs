//! Laravel's string-keyed names: config keys, route and view names,
//! container bindings, gate abilities, morph aliases, Artisan commands and
//! macros, each written as a string literal the symbol map makes navigable.
//!
//! Each file covers one family.  `spans` holds the primitive they all emit
//! through, and `receivers` the receiver spellings that gate them.

use super::*;

mod attributes;
mod commands;
mod config;
mod container;
mod facade_calls;
mod gates;
mod macros;
mod method_calls;
mod morph;
mod receivers;
mod routes;
mod spans;
mod views;

pub(in crate::symbol_map::extraction) use attributes::*;
pub(in crate::symbol_map::extraction) use commands::*;
pub(in crate::symbol_map::extraction) use config::*;
pub(in crate::symbol_map::extraction) use container::*;
pub(in crate::symbol_map::extraction) use facade_calls::*;
pub(in crate::symbol_map::extraction) use gates::*;
pub(in crate::symbol_map::extraction) use macros::*;
pub(in crate::symbol_map::extraction) use method_calls::*;
pub(in crate::symbol_map::extraction) use morph::*;
pub(in crate::symbol_map::extraction) use receivers::*;
pub(in crate::symbol_map::extraction) use routes::*;
pub(in crate::symbol_map::extraction) use spans::*;
pub(in crate::symbol_map::extraction) use views::*;

pub(crate) use config::{config_keys_written_by, is_config_repository_method};
pub(crate) use container::is_laravel_container_expr;
pub(crate) use facade_calls::{
    argument_takes_name_list, facade_static_call_arg_kind, helper_function_accepts_name_list,
    helper_function_first_arg_kind,
};
pub(crate) use gates::is_gate_check_method;
pub(crate) use method_calls::{MethodReceiver, instance_method_arg_kind};
pub(crate) use morph::{
    MORPH_TYPES_ARG, is_morph_map_registration_method, is_morph_types_query_method,
};
pub(crate) use receivers::chain_roots_at_facade;
pub(crate) use routes::{
    ROUTE_HTTP_METHODS, is_request_route_pattern_method, is_route_facade_pattern_method,
    is_route_name_method,
};
pub(crate) use views::{is_render_each_method, view_factory_names_view_at};
