//! Byte-offset guard collection for the undefined-variable diagnostic.
//!
//! These helpers scan a function/method body (or its raw source text)
//! for byte offsets that the diagnostic must treat specially: reads
//! guarded by `isset()`/`empty()`, reads under the `@` error
//! suppression operator, and `/** @var Type $var */` inline docblock
//! annotations, which act as a write at the annotation's offset rather
//! than a guard.
//!
//! The AST-based collectors are [`Walker`] visitors. Nested closures,
//! arrow functions, and named function declarations are their own
//! variable scopes, so the visitors stop at those boundaries; an
//! anonymous class still has its constructor arguments walked, since
//! those live in the enclosing scope.

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use crate::atom::bytes_to_str;

/// Emit [`Walker`] overrides that stop traversal at nested variable
/// scopes (closures, arrow functions, named function declarations) while
/// still walking an anonymous class's constructor arguments, which belong
/// to the enclosing scope.
macro_rules! stop_at_inner_scopes {
    ($ctx:ty) => {
        fn walk_closure(&self, _node: &'ast Closure<'arena>, _context: &mut $ctx) {}
        fn walk_arrow_function(&self, _node: &'ast ArrowFunction<'arena>, _context: &mut $ctx) {}
        fn walk_function(&self, _node: &'ast Function<'arena>, _context: &mut $ctx) {}
        fn walk_anonymous_class(&self, node: &'ast AnonymousClass<'arena>, context: &mut $ctx) {
            if let Some(argument_list) = &node.argument_list {
                self.walk_partial_argument_list(argument_list, context);
            }
        }
    };
}

// ─── @var annotation collection ─────────────────────────────────────────────

/// Scan the source text for `/** @var Type $varName */` inline
/// docblocks and return each declared variable name paired with the byte
/// offset of its `$` sigil.
///
/// The offset lets callers treat the annotation as a write at that
/// position so it (a) only defines the variable within the scope it
/// appears in, and (b) follows the same "prior write in source order"
/// rule as ordinary assignments.
pub(super) fn collect_var_annotations(content: &str) -> Vec<(String, u32)> {
    let mut vars = Vec::new();
    // Look for patterns like: @var SomeType $varName
    // The regex-like scan: find `@var ` followed by a type, then `$name`.
    let mut line_start = 0usize;
    for line in content.lines() {
        // `lines()` strips the line terminator; track the running byte
        // offset so we can report absolute positions.
        let this_line_start = line_start;
        line_start += line.len() + 1; // +1 for the stripped '\n'

        if !line.contains("@var") {
            continue;
        }
        // Find `@var` and extract the variable name after the type.
        if let Some(var_pos) = line.find("@var") {
            let after_var_off = var_pos + 4;
            let after_var = &line[after_var_off..];
            let ws = after_var.len() - after_var.trim_start().len();
            let after_var = after_var.trim_start();
            // Skip the type (everything before the $).
            if let Some(dollar_pos) = after_var.find('$') {
                let var_part = &after_var[dollar_pos..];
                // Extract the variable name: $[a-zA-Z_][a-zA-Z0-9_]*
                let name_end = var_part
                    .char_indices()
                    .skip(1) // skip the $
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map(|(i, _)| i)
                    .unwrap_or(var_part.len());
                let var_name = &var_part[..name_end];
                // Trim trailing `*/` if present.
                let var_name = var_name.trim_end_matches("*/").trim();
                if var_name.len() > 1 {
                    let dollar_offset = this_line_start + after_var_off + ws + dollar_pos;
                    vars.push((var_name.to_string(), dollar_offset as u32));
                }
            }
        }
    }
    vars
}

// ─── Error suppression (@) offset collection ────────────────────────────────

/// Collect byte offsets of variable reads that appear under the `@` error
/// suppression operator (e.g. `@$var`, `@foo($var)`).
pub(super) fn collect_error_suppressed_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let walker = SuppressedWalker;
    let mut ctx = SuppressedCtx {
        offsets: HashSet::new(),
        error_depth: 0,
    };
    for stmt in statements {
        walker.walk_statement(stmt, &mut ctx);
    }
    ctx.offsets
}

struct SuppressedCtx {
    offsets: HashSet<u32>,
    error_depth: u32,
}

struct SuppressedWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, SuppressedCtx> for SuppressedWalker {
    fn walk_unary_prefix(&self, node: &'ast UnaryPrefix<'arena>, context: &mut SuppressedCtx) {
        if node.operator.is_error_control() {
            context.error_depth += 1;
            self.walk_expression(node.operand, context);
            context.error_depth -= 1;
        } else {
            self.walk_expression(node.operand, context);
        }
    }

    fn walk_in_direct_variable(
        &self,
        node: &'ast DirectVariable<'arena>,
        context: &mut SuppressedCtx,
    ) {
        if context.error_depth > 0 {
            context.offsets.insert(node.span().start.offset);
        }
    }

    stop_at_inner_scopes!(SuppressedCtx);
}

// ─── isset() / empty() guarded offset collection ───────────────────────────

/// Collect byte offsets of variable reads that appear inside `isset()` or
/// `empty()` calls.  These variables are being guarded, not used.
pub(super) fn collect_guarded_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let walker = GuardedWalker;
    let mut offsets = HashSet::new();
    for stmt in statements {
        walker.walk_statement(stmt, &mut offsets);
    }
    offsets
}

struct GuardedWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, HashSet<u32>> for GuardedWalker {
    fn walk_in_isset_construct(
        &self,
        node: &'ast IssetConstruct<'arena>,
        context: &mut HashSet<u32>,
    ) {
        for value in node.values.iter() {
            collect_guard_targets(value, context);
        }
    }

    fn walk_in_empty_construct(
        &self,
        node: &'ast EmptyConstruct<'arena>,
        context: &mut HashSet<u32>,
    ) {
        collect_guard_targets(node.value, context);
    }

    stop_at_inner_scopes!(HashSet<u32>);
}

/// Collect all variable offsets within an expression that is a target
/// of `isset()` or `empty()`.  This handles simple variables,
/// array access chains (`$arr['key']`), and property chains
/// (`$obj->prop`).
fn collect_guard_targets(expr: &Expression<'_>, offsets: &mut HashSet<u32>) {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => {
            offsets.insert(dv.span().start.offset);
        }
        Expression::ArrayAccess(aa) => {
            collect_guard_targets(aa.array, offsets);
            // Don't mark the index expression as guarded.
        }
        Expression::Access(Access::Property(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::NullSafeProperty(pa)) => {
            collect_guard_targets(pa.object, offsets);
        }
        Expression::Access(Access::StaticProperty(spa)) => {
            collect_guard_targets(spa.class, offsets);
        }
        _ => {}
    }
}

// ─── Short-circuit `isset()` guard collection ──────────────────────────────

/// Collect byte offsets of variable reads that are guarded by an
/// `isset()`/`!isset()` check earlier in the same short-circuiting `&&`
/// or `||` chain, e.g. `isset($x) && $x > 1` or `!isset($x) || $x > 1`.
/// The right-hand side only evaluates once the left-hand side has
/// established that the variable exists, so a read there is not a use
/// of an undefined variable.
///
/// This only covers the operands of the boolean expression itself, not
/// the surrounding `if`/`while` body — a plain `isset($x)` check does
/// not otherwise define `$x` (see `flags_undefined_variable_after_isset_guard`).
pub(super) fn collect_short_circuit_guarded_offsets(statements: &[Statement<'_>]) -> HashSet<u32> {
    let walker = ShortCircuitWalker;
    let mut offsets = HashSet::new();
    for stmt in statements {
        walker.walk_statement(stmt, &mut offsets);
    }
    offsets
}

struct ShortCircuitWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, HashSet<u32>> for ShortCircuitWalker {
    fn walk_in_binary(&self, node: &'ast Binary<'arena>, context: &mut HashSet<u32>) {
        let is_or = match node.operator {
            BinaryOperator::And(_) | BinaryOperator::LowAnd(_) => false,
            BinaryOperator::Or(_) | BinaryOperator::LowOr(_) => true,
            _ => return,
        };

        let mut operands = Vec::new();
        collect_chain_operands(node.lhs, is_or, &mut operands);
        collect_chain_operands(node.rhs, is_or, &mut operands);
        if operands.len() < 2 {
            return;
        }

        // `&&` only guards later operands with a positive `isset()`;
        // `||` only guards later operands with a negated `!isset()`
        // (the right-hand side runs only when the left-hand side, and
        // therefore the isset() check, was false).
        let mut guarded: HashSet<String> = HashSet::new();
        for operand in &operands {
            mark_guarded_reads(operand, &guarded, context);
            for name in isset_guard_names(operand, is_or) {
                guarded.insert(name);
            }
        }
    }

    stop_at_inner_scopes!(HashSet<u32>);
}

/// Flatten a left-associative chain of the same short-circuiting
/// operator (`&&`/`and` when `is_or` is `false`, `||`/`or` otherwise)
/// into its individual operands, unwrapping parentheses around nested
/// chains of the same operator.
fn collect_chain_operands<'e>(
    expr: &'e Expression<'e>,
    is_or: bool,
    out: &mut Vec<&'e Expression<'e>>,
) {
    if let Expression::Binary(bin) = expr {
        let matches_operator = if is_or {
            matches!(
                bin.operator,
                BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
            )
        } else {
            matches!(
                bin.operator,
                BinaryOperator::And(_) | BinaryOperator::LowAnd(_)
            )
        };
        if matches_operator {
            collect_chain_operands(bin.lhs, is_or, out);
            collect_chain_operands(bin.rhs, is_or, out);
            return;
        }
    }
    if let Expression::Parenthesized(inner) = expr {
        let mut inner_operands = Vec::new();
        collect_chain_operands(inner.expression, is_or, &mut inner_operands);
        if inner_operands.len() > 1 {
            out.extend(inner_operands);
            return;
        }
    }
    out.push(expr);
}

/// Unwrap parentheses and a single `!` prefix from a condition,
/// returning `(inner_expr, negated)`.
fn unwrap_negation<'e>(expr: &'e Expression<'e>) -> (&'e Expression<'e>, bool) {
    match expr {
        Expression::Parenthesized(inner) => unwrap_negation(inner.expression),
        Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
            let (inner, already_negated) = unwrap_negation(prefix.operand);
            (inner, !already_negated)
        }
        _ => (expr, false),
    }
}

/// If `expr` (after unwrapping parens/`!`) is an `isset()` call whose
/// negation matches `want_negated`, return the base variable names of
/// its guard targets.
fn isset_guard_names(expr: &Expression<'_>, want_negated: bool) -> Vec<String> {
    let (inner, negated) = unwrap_negation(expr);
    if negated != want_negated {
        return Vec::new();
    }
    let Expression::Construct(Construct::Isset(isset)) = inner else {
        return Vec::new();
    };
    isset
        .values
        .iter()
        .filter_map(|value| base_variable_name(value))
        .collect()
}

/// Resolve the base variable name of an `isset()` guard target, e.g.
/// `$arr` for `$arr['key']` or `$obj` for `$obj->prop`.
fn base_variable_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Variable(Variable::Direct(dv)) => Some(bytes_to_str(dv.name).to_string()),
        Expression::ArrayAccess(aa) => base_variable_name(aa.array),
        Expression::Access(Access::Property(pa)) => base_variable_name(pa.object),
        Expression::Access(Access::NullSafeProperty(pa)) => base_variable_name(pa.object),
        Expression::Access(Access::StaticProperty(spa)) => base_variable_name(spa.class),
        _ => None,
    }
}

/// Mark the byte offsets of every read of a variable named in `names`
/// found anywhere within `expr`.
fn mark_guarded_reads(expr: &Expression<'_>, names: &HashSet<String>, offsets: &mut HashSet<u32>) {
    if names.is_empty() {
        return;
    }
    let walker = VarNameOffsetWalker;
    let mut ctx = VarNameOffsetCtx {
        names,
        offsets: HashSet::new(),
    };
    walker.walk_expression(expr, &mut ctx);
    offsets.extend(ctx.offsets);
}

struct VarNameOffsetCtx<'n> {
    names: &'n HashSet<String>,
    offsets: HashSet<u32>,
}

struct VarNameOffsetWalker;

impl<'ast, 'arena, 'n> Walker<'ast, 'arena, VarNameOffsetCtx<'n>> for VarNameOffsetWalker {
    fn walk_in_direct_variable(
        &self,
        node: &'ast DirectVariable<'arena>,
        context: &mut VarNameOffsetCtx<'n>,
    ) {
        if context.names.contains(bytes_to_str(node.name)) {
            context.offsets.insert(node.span().start.offset);
        }
    }

    stop_at_inner_scopes!(VarNameOffsetCtx<'n>);
}
