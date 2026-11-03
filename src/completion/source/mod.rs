//! Source analysis sub-modules.
//!
//! This group contains modules for source-text scanning and position
//! detection utilities:
//! - **code_context**: The lexical position of an offset (`code_context_at`) —
//!   the string literal it sits in, the code that precedes it, and the
//!   brackets left open at it
//! - **comment_position**: Comment, docblock, and string position detection
//!   (`is_inside_docblock`, `is_inside_non_doc_comment`, `classify_string_context`)
//! - **helpers**: Source-text scanning helpers (closure return types,
//!   first-class callable resolution, `new` expression parsing, array access)
//! - **throws_analysis**: Throws analysis pipeline (throw scanning, catch-block
//!   filtering, uncaught detection, method `@throws` / return-type lookup)

pub(crate) mod code_context;
pub mod comment_position;
pub(crate) mod helpers;
pub(crate) mod string_argument;
pub(crate) mod throws_analysis;
