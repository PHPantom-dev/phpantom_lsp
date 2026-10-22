//! `Backend` behaviour that doesn't belong in the LSP-dispatch layer
//! (`server.rs`) or in a specific feature module.

pub(crate) mod accessors;
mod client_caps;
pub(crate) mod client_refresh;
pub(crate) mod construction;
pub(crate) mod documents;
pub(crate) mod eager_population;
pub(crate) mod external_tool_worker;
pub(crate) mod file_access;
mod gated_index;
pub(crate) mod laravel;
pub(crate) mod package_origin;
pub(crate) mod requests;
pub(crate) mod settings;
pub(crate) mod startup;
mod workspace_index_state;

pub(crate) use client_caps::ClientCaps;
pub(crate) use gated_index::GatedIndex;
pub(crate) use workspace_index_state::WorkspaceIndexState;
