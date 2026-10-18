//! Variable-completion sub-modules.
//!
//! - **completion**: Variable name completions and scope collection
//!
//! Variable *type* resolution lives in the shared type engine under
//! `crate::type_engine::variable`, not here.

pub(crate) mod completion;
