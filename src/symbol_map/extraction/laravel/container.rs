//! Service container binding keys: `app()->make('payments')` asks for one
//! and `$this->app->singleton('payments', …)` registers it.

use super::*;

/// Whether `object` names the service container the way source spells it:
/// `$this->app` inside a service provider, the `$app` a container factory
/// receives, or a bare `app()` / `\app()` call.
///
/// A receiver whose containerness is only knowable from its type (an injected
/// `Container $container`) is left out: the spelling has to say so, the same
/// bar every other string kind extracted here is held to.
pub(crate) fn is_laravel_container_expr(object: &Expression<'_>) -> bool {
    match object {
        Expression::Variable(Variable::Direct(var)) => var.name == b"$app",
        Expression::Access(Access::Property(access)) => {
            matches!(access.object, Expression::Variable(Variable::Direct(var)) if var.name == b"$this")
                && matches!(
                    &access.property,
                    ClassLikeMemberSelector::Identifier(ident)
                        if ident.value.eq_ignore_ascii_case(b"app")
                )
        }
        Expression::Call(Call::Function(func_call)) => {
            matches!(func_call.function, Expression::Identifier(ident)
                if strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("app"))
                && func_call.argument_list.arguments.is_empty()
        }
        _ => false,
    }
}

/// Where a container method names its binding key, and whether the call
/// *registers* the key rather than asking for it.
///
/// `extend()` decorates whatever the key already holds, so it needs the key
/// to exist rather than introducing it.  `alias()` is the odd one out the
/// other way: it names the key it introduces second, after the binding that
/// key stands for.
pub(in crate::symbol_map::extraction) fn container_key_argument(
    member_name: &str,
) -> Option<(usize, bool)> {
    match member_name.to_ascii_lowercase().as_str() {
        "make" | "makewith" | "resolve" | "bound" | "resolved" | "extend" => Some((0, false)),
        "bind" | "bindif" | "singleton" | "singletonif" | "scoped" | "scopedif" | "instance" => {
            Some((0, true))
        }
        "alias" => Some((1, true)),
        _ => None,
    }
}

/// Emit the container-key span of a call that names one, whichever argument
/// holds it.
pub(in crate::symbol_map::extraction) fn try_emit_container_key_span(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some((index, is_write)) = container_key_argument(member_name) else {
        return;
    };
    emit_laravel_string_span(
        crate::symbol_map::StringKeyKind::ContainerBinding,
        is_write,
        index,
        argument_list,
        content,
        spans,
    );
}
