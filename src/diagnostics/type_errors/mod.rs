//! Argument type mismatch diagnostics.
//!
//! Walk the precomputed [`CallSite`] entries in the symbol map and flag
//! every call where an argument's resolved type is incompatible with
//! the declared parameter type (see [`arguments`]).
//!
//! This is Phase 1 of the type error diagnostic suite. Only clearly
//! incompatible types are flagged — when in doubt (unresolved types,
//! `mixed`, complex generics), the diagnostic is suppressed to avoid
//! false positives.
//!
//! [`CallSite`]: crate::symbol_map::CallSite

mod arguments;
pub(super) mod compatibility;

pub(super) use compatibility::is_type_compatible;

use mago_syntax::cst::Program;
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::statement::Statement;

use crate::atom::bytes_to_str;

/// Diagnostic code used for argument type mismatch diagnostics.
pub(crate) const TYPE_MISMATCH_ARGUMENT_CODE: &str = "type_mismatch_argument";

/// Returns `true` when the file declares `strict_types=1`.
///
/// Scans the top-level statements of the parsed program for a
/// `declare(strict_types=1)` directive.  In PHP this must appear as
/// the very first statement (after `<?php`), but we check all
/// top-level statements for robustness.
pub(super) fn has_strict_types(program: &Program<'_>) -> bool {
    for stmt in program.statements.iter() {
        if let Statement::Declare(declare) = stmt {
            for item in declare.items.iter() {
                if bytes_to_str(item.name.value).eq_ignore_ascii_case("strict_types")
                    && let Expression::Literal(Literal::Integer(i)) = item.value
                    && bytes_to_str(i.raw) == "1"
                {
                    return true;
                }
            }
        }
    }
    false
}
