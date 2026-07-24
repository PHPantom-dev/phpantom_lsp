use mago_syntax::cst::*;

use super::*;

// ─── Language constructs ─────────────────────────────────────────────────────
// `isset($a, $b)`, `empty($x)`, `eval(...)`, `print(...)`,
// `include ...`, `require ...`, `exit(...)`, `die(...)`

pub(super) fn extract_construct_expr<'a>(
    construct: &'a Construct<'a>,
    ctx: &mut ExtractionCtx<'a>,
    scope_start: u32,
) {
    match construct {
        Construct::Isset(isset) => {
            emit_keyword(&isset.isset, ctx);
            for val in isset.values.iter() {
                extract_from_expression(val, ctx, scope_start);
            }
        }
        Construct::Empty(empty) => {
            emit_keyword(&empty.empty, ctx);
            extract_from_expression(empty.value, ctx, scope_start);
        }
        Construct::Eval(eval) => {
            emit_keyword(&eval.eval, ctx);
            extract_from_expression(eval.value, ctx, scope_start);
        }
        Construct::Include(inc) => {
            emit_keyword(&inc.include, ctx);
            extract_from_expression(inc.value, ctx, scope_start);
        }
        Construct::IncludeOnce(inc) => {
            emit_keyword(&inc.include_once, ctx);
            extract_from_expression(inc.value, ctx, scope_start);
        }
        Construct::Require(req) => {
            emit_keyword(&req.require, ctx);
            extract_from_expression(req.value, ctx, scope_start);
        }
        Construct::RequireOnce(req) => {
            emit_keyword(&req.require_once, ctx);
            extract_from_expression(req.value, ctx, scope_start);
        }
        Construct::Print(print) => {
            emit_keyword(&print.print, ctx);
            extract_from_expression(print.value, ctx, scope_start);
        }
        Construct::Exit(exit) => {
            emit_keyword(&exit.exit, ctx);
            if let Some(ref args) = exit.arguments {
                extract_from_arguments(&args.arguments, ctx, scope_start);
            }
        }
        Construct::Die(die) => {
            emit_keyword(&die.die, ctx);
            if let Some(ref args) = die.arguments {
                extract_from_arguments(&args.arguments, ctx, scope_start);
            }
        }
    }
}
