//! `Backend` behaviour that doesn't belong in the LSP-dispatch layer
//! (`server.rs`) or in a specific feature module.

pub(crate) mod eager_population;
pub(crate) mod file_access;
pub(crate) mod laravel_scan;
pub(crate) mod requests;
