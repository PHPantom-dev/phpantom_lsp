//! View paths passed to Tempest's fully qualified `view()` helper.

use super::*;
use crate::symbol_map::StringKeyKind;

pub(super) fn emit_view_path(
    call: &FunctionCall<'_>,
    name: &Identifier<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let resolved = ctx
        .resolved_name_at(name.span().start.offset)
        .unwrap_or_else(|| bytes_to_str(name.value()));
    if !resolved
        .trim_start_matches('\\')
        .eq_ignore_ascii_case("Tempest\\View\\view")
    {
        return;
    }
    let argument =
        call.argument_list
            .arguments
            .iter()
            .enumerate()
            .find_map(|(index, arg)| match arg {
                Argument::Named(named) if named.name.value == b"path" => Some(named.value),
                Argument::Positional(arg) if index == 0 => Some(arg.value),
                _ => None,
            });
    let Some(expression) = argument else {
        return;
    };
    let (expression, relative) = match expression {
        Expression::Binary(binary)
            if binary.operator.is_concatenation()
                && matches!(
                    binary.lhs,
                    Expression::MagicConstant(MagicConstant::Directory(_))
                ) =>
        {
            (binary.rhs, true)
        }
        expression => (expression, false),
    };
    let Expression::Literal(Literal::String(literal)) = expression else {
        return;
    };
    let Some(value) = literal.value.and_then(literal_bytes_to_str) else {
        return;
    };
    ctx.spans.push(SymbolSpan {
        start: literal.span.start.offset + 1,
        end: literal.span.end.offset.saturating_sub(1),
        kind: SymbolKind::StringKey {
            kind: StringKeyKind::TempestView { relative },
            key: value.to_string(),
            is_write: false,
            is_optional: false,
        },
    });
}
