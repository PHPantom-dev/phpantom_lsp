//! How a receiver's spelling says it is a Laravel service: the facade a
//! static call is written against, a method chain rooted at a facade, and
//! the bare helper calls (`view()`, `redirect()`) that hand back the service
//! they wrap.

use super::*;

/// Whether a semantically resolved class is either Laravel's global runtime
/// alias or the exact class inside `namespace`.
pub(in crate::symbol_map::extraction) fn matches_laravel_class(
    class_name: &str,
    namespace: &str,
    short: &str,
) -> bool {
    let class_name = class_name.trim_start_matches('\\');
    if class_name.eq_ignore_ascii_case(short) {
        return true;
    }
    class_name
        .rsplit_once('\\')
        .is_some_and(|(actual_namespace, actual_short)| {
            actual_namespace.eq_ignore_ascii_case(namespace)
                && actual_short.eq_ignore_ascii_case(short)
        })
}

/// Whether a semantically resolved class names one Laravel facade.
pub(in crate::symbol_map::extraction) fn matches_laravel_facade(
    class_name: &str,
    short: &str,
) -> bool {
    matches_laravel_class(class_name, "Illuminate\\Support\\Facades", short)
}

/// Whether `object` is a bare `name()` (or `\name()`) helper call: the
/// argument-less spelling, which hands back the service the helper wraps
/// rather than a result built from what it was passed.
pub(super) fn is_bare_helper_call(object: &Expression<'_>, name: &str) -> bool {
    let Expression::Call(Call::Function(func_call)) = object else {
        return false;
    };
    let Expression::Identifier(ident) = func_call.function else {
        return false;
    };
    strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case(name)
        && func_call.argument_list.arguments.is_empty()
}

/// Walk at most `depth` links down a method chain looking for a static call
/// whose class satisfies `is_facade`.
pub(crate) fn chain_roots_at_facade(
    expr: &Expression<'_>,
    depth: usize,
    is_facade: &dyn Fn(&str) -> bool,
) -> bool {
    if depth == 0 {
        return false;
    }
    match expr {
        Expression::Call(Call::Method(mc)) => {
            chain_roots_at_facade(mc.object, depth - 1, is_facade)
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            is_facade(strip_fqn_prefix(&expr_to_subject_text(sc.class)))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_matching_accepts_only_runtime_aliases_and_laravel_facades() {
        assert!(matches_laravel_facade("Storage", "Storage"));
        assert!(matches_laravel_facade(
            "Illuminate\\Support\\Facades\\Storage",
            "Storage"
        ));
        assert!(!matches_laravel_facade("App\\Storage", "Storage"));
        assert!(!matches_laravel_facade("Acme\\Storage", "Storage"));
    }
}
