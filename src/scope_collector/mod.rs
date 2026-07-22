//! ScopeCollector — forward-pass variable read/write analysis.
//!
//! This module provides a lightweight forward-pass AST walker that
//! collects every variable read and write with byte offsets across a
//! function/method/closure body.  It is shared infrastructure used by:
//!
//! - Extract Function
//! - Inline Variable
//! - Extract Variable
//! - Inline Function/Method
//! - Extract Constant
//! - Undefined variable diagnostic
//! - Document highlights (all occurrences of a variable in scope)
//!
//! Unlike the existing backward-walk variable resolution in
//! `completion/variable/resolution.rs` (which resolves the type of a
//! single variable at a specific cursor position), the `ScopeCollector`
//! walks **forward** through an entire function body and records _all_
//! variable definitions and usages.
//!
//! # Key concepts
//!
//! - **Frame** = scope boundary.  Each function body, closure, arrow
//!   function, and `catch` block opens a new frame.  Variables defined
//!   inside a frame are local to it.  Closures capture via `use()`;
//!   arrow functions capture by value.  `foreach`, `if`, `for` blocks
//!   do _not_ open new frames in PHP — variables leak into the
//!   enclosing scope.
//!
//! - **VarAccess** = a single read or write of a variable, with name,
//!   byte offset, and access kind.
//!
//! - **ScopeMap** = the result of collecting.  Contains all accesses
//!   organised by frame, plus a query API for extracting parameter sets,
//!   return value sets, and local sets for a given byte range.

mod build;
mod collector;
mod scope_map;

#[cfg(test)]
mod tests;

pub(crate) use build::{
    build_scope_map_for_offset, collect_function_scope, collect_function_scope_with_kind,
    collect_function_scope_with_kind_and_resolver, collect_function_scope_with_resolver,
};
pub(crate) use scope_map::{AccessKind, ByRefCallKind, ByRefResolver, Frame, FrameKind, ScopeMap};
