//! Symfony-specific adapters.
//!
//! The modules here recover framework runtime wiring behind small metadata
//! interfaces. Package-specific attributes and naming rules remain project
//! configuration rather than constants in the language server.

pub(crate) mod container;
mod events;
mod expressions;
mod php_attributes;

pub(crate) use events::SymfonyEventIndex;
