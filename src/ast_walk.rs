//! Stack-safe overrides for `mago_syntax`'s generated AST walkers.
//!
//! The default [`Walker::walk_binary`] walks an operator's left operand
//! before its right, recursing into the next operator down the chain.
//! Binary operators nest on their left (`"a" . "b" . "c"` is
//! `("a" . "b") . "c"`), so a chain costs a few stack frames per operator,
//! and generated code can chain tens of thousands of them.  Every walker
//! that can be handed user code invokes [`iterative_binary_walk!`] in its
//! `impl` to walk a chain's left spine in a loop instead.  A walker that
//! overrides `walk_binary` itself hands the operators it does not handle to
//! [`walk_binary_descending`].

use mago_syntax::cst::{Binary, Expression};
use mago_syntax::walker::{MutWalker, Walker};

/// Emit a `walk_binary` override that walks operator chains without a
/// stack frame per operator, by calling [`walk_binary`].
///
/// Pass the walker's context type, and its two lifetimes when the `impl`
/// does not name them `'ast` and `'arena`.
macro_rules! iterative_binary_walk {
    ($ast:lifetime, $arena:lifetime, $ctx:ty) => {
        fn walk_binary(
            &self,
            binary: &$ast mago_syntax::cst::Binary<$arena>,
            context: &mut $ctx,
        ) {
            $crate::ast_walk::walk_binary(self, binary, context);
        }
    };
    ($ctx:ty) => {
        $crate::ast_walk::iterative_binary_walk!('ast, 'arena, $ctx);
    };
}
pub(crate) use iterative_binary_walk;

/// The [`MutWalker`] counterpart of [`iterative_binary_walk!`].
macro_rules! iterative_binary_walk_mut {
    ($ctx:ty) => {
        fn walk_binary(
            &mut self,
            binary: &'ast mago_syntax::cst::Binary<'arena>,
            context: &mut $ctx,
        ) {
            $crate::ast_walk::walk_binary_mut(self, binary, context);
        }
    };
}
pub(crate) use iterative_binary_walk_mut;

/// Walk `binary` with exactly the hook calls mago's default
/// `walk_binary` makes, in the same order, but loop down its left spine
/// rather than recursing into each operator's left operand.
pub(crate) fn walk_binary<'ast, 'arena, C, W>(
    walker: &W,
    binary: &'ast Binary<'arena>,
    context: &mut C,
) where
    W: Walker<'ast, 'arena, C> + ?Sized,
{
    walk_binary_descending(walker, binary, context, |_| true);
}

/// [`walk_binary`] for a walker that overrides `walk_binary` for some
/// operators: the loop only descends into the operators `descend` accepts,
/// and hands any other one on the spine to the walker's own
/// `walk_expression`, so it reaches the override.
pub(crate) fn walk_binary_descending<'ast, 'arena, C, W>(
    walker: &W,
    binary: &'ast Binary<'arena>,
    context: &mut C,
    descend: impl Fn(&Binary<'arena>) -> bool,
) where
    W: Walker<'ast, 'arena, C> + ?Sized,
{
    walker.walk_in_binary(binary, context);
    // The operators below `binary` on its left spine, each with the
    // expression that wraps it, outermost first.
    let mut spine: Vec<(&'ast Expression<'arena>, &'ast Binary<'arena>)> = Vec::new();
    let mut leftmost: &'ast Expression<'arena> = binary.lhs;
    while let Expression::Binary(inner) = leftmost
        && descend(inner)
    {
        walker.walk_in_expression(leftmost, context);
        walker.walk_in_binary(inner, context);
        spine.push((leftmost, inner));
        leftmost = inner.lhs;
    }
    walker.walk_expression(leftmost, context);
    for (expression, inner) in spine.into_iter().rev() {
        walker.walk_binary_operator(&inner.operator, context);
        walker.walk_expression(inner.rhs, context);
        walker.walk_out_binary(inner, context);
        walker.walk_out_expression(expression, context);
    }
    walker.walk_binary_operator(&binary.operator, context);
    walker.walk_expression(binary.rhs, context);
    walker.walk_out_binary(binary, context);
}

/// The [`MutWalker`] counterpart of [`walk_binary`].
pub(crate) fn walk_binary_mut<'ast, 'arena, C, W>(
    walker: &mut W,
    binary: &'ast Binary<'arena>,
    context: &mut C,
) where
    W: MutWalker<'ast, 'arena, C> + ?Sized,
{
    walker.walk_in_binary(binary, context);
    let mut spine: Vec<(&'ast Expression<'arena>, &'ast Binary<'arena>)> = Vec::new();
    let mut leftmost: &'ast Expression<'arena> = binary.lhs;
    while let Expression::Binary(inner) = leftmost {
        walker.walk_in_expression(leftmost, context);
        walker.walk_in_binary(inner, context);
        spine.push((leftmost, inner));
        leftmost = inner.lhs;
    }
    walker.walk_expression(leftmost, context);
    for (expression, inner) in spine.into_iter().rev() {
        walker.walk_binary_operator(&inner.operator, context);
        walker.walk_expression(inner.rhs, context);
        walker.walk_out_binary(inner, context);
        walker.walk_out_expression(expression, context);
    }
    walker.walk_binary_operator(&binary.operator, context);
    walker.walk_expression(binary.rhs, context);
    walker.walk_out_binary(binary, context);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mago_allocator::LocalArena;
    use mago_database::file::FileId;

    /// Records every hook the walk fires, so the iterative walk can be
    /// compared with mago's recursive one.
    struct Recorder;

    macro_rules! record {
        ($($hook:ident: $node:ident,)*) => {
            $(
                fn $hook(&self, node: &'ast mago_syntax::cst::$node<'arena>, log: &mut Vec<String>) {
                    use mago_span::HasSpan;
                    log.push(format!("{} {}", stringify!($hook), node.span().start.offset));
                }
            )*
        };
    }

    impl<'ast, 'arena> Walker<'ast, 'arena, Vec<String>> for Recorder {
        record! {
            walk_in_expression: Expression,
            walk_out_expression: Expression,
            walk_in_binary: Binary,
            walk_out_binary: Binary,
            walk_in_binary_operator: BinaryOperator,
            walk_in_variable: Variable,
        }
    }

    struct IterativeRecorder;

    impl<'ast, 'arena> Walker<'ast, 'arena, Vec<String>> for IterativeRecorder {
        record! {
            walk_in_expression: Expression,
            walk_out_expression: Expression,
            walk_in_binary: Binary,
            walk_out_binary: Binary,
            walk_in_binary_operator: BinaryOperator,
            walk_in_variable: Variable,
        }
        iterative_binary_walk!(Vec<String>);
    }

    #[test]
    fn iterative_walk_fires_the_same_hooks_as_the_recursive_one() {
        let arena = LocalArena::new();
        let source = b"<?php $a . $b . ($c + $d * $e) . $f && $g || ($h . $i) . $j;";
        let program =
            mago_syntax::parser::parse_file_content(&arena, FileId::new(b"input.php"), source);

        let mut recursive = Vec::new();
        Recorder.walk_program(program, &mut recursive);
        let mut iterative = Vec::new();
        IterativeRecorder.walk_program(program, &mut iterative);

        assert!(recursive.len() > 20, "the walk should record hooks");
        assert_eq!(iterative, recursive);
    }
}
