//! Precomputed symbol-location map for a single PHP file.
//!
//! During `update_ast`, every navigable symbol occurrence (class reference,
//! member access, variable, function call, etc.) is recorded as a
//! [`SymbolSpan`] in a flat, sorted vec.  At request time a binary search
//! on this vec replaces character-level backward-walking and
//! provides instant rejection when the cursor lands on whitespace, a
//! string literal, a comment, or any other non-navigable token.
//!
//! The map also stores variable definition sites ([`VarDefSite`]) and
//! scope boundaries so that go-to-definition for `$variable` can be
//! answered entirely from precomputed data without re-parsing.
//!
//! Docblock type references (from `@param`, `@return`, `@var`,
//! `@template`, `@method`, etc.) are extracted by parsing each
//! docblock's own PHPDoc CST during the AST walk, since docblocks are
//! trivia in the `mago_syntax` AST and produce no expression/statement
//! nodes.
//!
//! The module is split into submodules:
//!
//! - [`docblock`] — Docblock symbol extraction helpers (type span
//!   emission, `@template` / `@method` tag scanning, navigability
//!   filtering, and `get_docblock_text_with_offset`)
//! - [`extraction`] — AST walk that builds a [`SymbolMap`] from a
//!   parsed PHP program (`extract_symbol_map` and all
//!   `extract_from_*` helpers)
//! - [`call_sites`] — the call sites signature help reads, stored in
//!   shared arenas
//! - [`laravel_spans`] — Laravel string keys and the spans that depend
//!   on a workspace symbol being absent
//! - [`span`] — the symbol spans and their kinds
//! - [`map`] — [`SymbolMap`] itself and its lookups

mod call_sites;
pub(crate) mod docblock;
pub(crate) mod extraction;
pub(crate) mod laravel_resources;
mod laravel_spans;
mod map;
mod morph_columns;
mod span;

pub(crate) use call_sites::*;
#[cfg(test)]
pub(crate) use extraction::extract_symbol_map;
pub(crate) use extraction::extract_symbol_map_for_index;
pub(crate) use laravel_spans::*;
pub(crate) use map::*;
pub(crate) use morph_columns::{MorphColumnReceiver, MorphColumnSite};
pub(crate) use span::*;

#[cfg(test)]
mod tests;
