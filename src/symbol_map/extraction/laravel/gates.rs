//! Authorization gate abilities: `Gate::define()` declares one, and the
//! checks (`Gate::allows()`, `$user->can()`, `$this->authorize()`, `@can`,
//! `can:` middleware) name one, along with the model the check is about.

use super::*;
use crate::virtual_members::laravel::helpers::string_literal_at_range;

/// What a recognised ability span is doing with the name it holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::symbol_map::extraction) enum AbilityRole {
    /// `Gate::define('name', …)` declares the ability, so the span is a
    /// declaration site rather than a use of one.
    Definition,
    /// A check that has to name a real ability.
    Check,
    /// `Gate::has('name')` asks whether the ability is registered at all, so
    /// a name that is not registered is the question rather than a mistake.
    Existence,
}

/// Methods on Laravel's `Gate` whose first argument is an ability name (or a
/// list of them), and whose second is the model the check is about.
///
/// Compared case-insensitively without lowercasing into a `String`: this runs
/// for every method call in every file.
pub(crate) fn is_gate_check_method(member_name: &str) -> bool {
    const GATE_CHECK_METHODS: &[&str] = &[
        "allows",
        "denies",
        "check",
        "any",
        "none",
        "authorize",
        "inspect",
        "has",
    ];
    GATE_CHECK_METHODS
        .iter()
        .any(|name| member_name.eq_ignore_ascii_case(name))
}

/// What a `Gate` method does with the ability it is handed.
///
/// `has()` is the framework's own "is this ability registered?" predicate, so
/// a name it does not find is the answer the call was written to get rather
/// than a typo.  The span is still emitted — completion, hover, and
/// go-to-definition all work inside it — but it is not judged.
pub(in crate::symbol_map::extraction) fn gate_ability_role(member_name: &str) -> AbilityRole {
    if member_name.eq_ignore_ascii_case("has") {
        AbilityRole::Existence
    } else {
        AbilityRole::Check
    }
}

/// Whether a static-call subject names Laravel's `Gate` facade.
pub(in crate::symbol_map::extraction) fn is_gate_facade(clean_subject: &str) -> bool {
    matches_laravel_facade(clean_subject, "Gate")
}

/// Whether an instance-method chain roots at the `Gate` facade, as
/// `Gate::forUser($user)->allows('update', $post)` does.
pub(in crate::symbol_map::extraction) fn chain_roots_at_gate(expr: &Expression<'_>) -> bool {
    chain_roots_at_facade(
        expr,
        crate::symbol_map::laravel_resources::FACADE_CHAIN_DEPTH,
        &|name| is_gate_facade(name),
    )
}

/// Whether a receiver plainly reads as the authenticated user, which is what
/// makes `->can('update', $post)` an authorization check rather than a
/// same-named method on an unrelated object.
///
/// Matches a variable whose name ends in `user` (`$user`, `$authUser`), a
/// property of that shape (`$this->user`), and any `user()` call
/// (`auth()->user()`, `Auth::user()`, `$request->user()`).  Anything else is
/// left alone: `can()` is too ordinary a method name to claim on sight.
pub(in crate::symbol_map::extraction) fn receiver_is_user_like(expr: &Expression<'_>) -> bool {
    fn ends_with_user(name: &str) -> bool {
        let name = name.trim_start_matches('$');
        let bytes = name.as_bytes();
        bytes.len() >= 4 && bytes[bytes.len() - 4..].eq_ignore_ascii_case(b"user")
    }

    match expr {
        Expression::Variable(Variable::Direct(dv)) => ends_with_user(bytes_to_str(dv.name)),
        Expression::Access(Access::Property(access)) => match &access.property {
            ClassLikeMemberSelector::Identifier(ident) => ends_with_user(bytes_to_str(ident.value)),
            _ => false,
        },
        Expression::Call(Call::Method(mc)) => match &mc.method {
            ClassLikeMemberSelector::Identifier(ident) => {
                bytes_to_str(ident.value).eq_ignore_ascii_case("user")
            }
            _ => false,
        },
        Expression::Call(Call::StaticMethod(sc)) => match &sc.method {
            ClassLikeMemberSelector::Identifier(ident) => {
                bytes_to_str(ident.value).eq_ignore_ascii_case("user")
            }
            _ => false,
        },
        _ => false,
    }
}

/// Emit a [`crate::symbol_map::LaravelStringKind::GateAbility`] span for the
/// argument at `ability_index`, which Laravel accepts as either a single
/// string or an array of strings, and record the model named at
/// `model_index` (when there is one) as the checked subject.
pub(in crate::symbol_map::extraction) fn try_emit_gate_ability_spans(
    argument_list: &ArgumentList<'_>,
    ability_index: usize,
    model_index: Option<usize>,
    role: AbilityRole,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
    gate_subjects: &mut Vec<crate::symbol_map::GateSubject>,
) {
    let Some(ability_arg) = argument_list.arguments.iter().nth(ability_index) else {
        return;
    };

    let first_span = spans.len();
    for_each_value(ability_arg.value(), |expr| {
        push_gate_ability_span(expr, role, content, spans)
    });
    if spans.len() == first_span {
        return;
    }

    let Some(model_index) = model_index else {
        return;
    };
    let Some(model_arg) = argument_list.arguments.iter().nth(model_index) else {
        return;
    };
    let Some((subject_text, is_static)) = gate_model_subject(model_arg.value(), content) else {
        return;
    };
    for span in &spans[first_span..] {
        gate_subjects.push(crate::symbol_map::GateSubject {
            ability_start: span.start,
            subject_text: subject_text.clone(),
            is_static,
        });
    }
}

/// Push a gate-ability span for a single string-literal expression.
fn push_gate_ability_span(
    expr: &Expression<'_>,
    role: AbilityRole,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    // An empty literal is rejected by the extractor, so whatever is
    // between the quotes is a name.
    let Some((ability, start, end)) = string_literal_at_range(expr, content) else {
        return;
    };
    spans.push(SymbolSpan {
        start,
        end,
        kind: SymbolKind::LaravelStringKey {
            kind: crate::symbol_map::LaravelStringKind::GateAbility,
            key: ability.to_string(),
            is_write: role == AbilityRole::Definition,
            is_optional: role == AbilityRole::Existence,
        },
    });
}

/// The subject text of a gate check's model argument: `Post` for
/// `Post::class` (a class name), `$post` for a variable, `Post` for
/// `new Post`.  Anything else names no model we can resolve.
fn gate_model_subject(
    expr: &Expression<'_>,
    content: &str,
) -> Option<(crate::symbol_map::SubjectText, bool)> {
    let span = expr.span();
    match expr {
        Expression::Access(Access::ClassConstant(cca)) if crate::parser::is_class_fetch(cca) => {
            let name = expr_to_subject_text(cca.class);
            let class_span = cca.class.span();
            (!name.is_empty()).then(|| {
                (
                    SubjectText::new(
                        name,
                        class_span.start.offset,
                        class_span.end.offset,
                        content,
                    ),
                    true,
                )
            })
        }
        Expression::Variable(Variable::Direct(dv)) => Some((
            SubjectText::new(
                bytes_to_str(dv.name).to_string(),
                span.start.offset,
                span.end.offset,
                content,
            ),
            false,
        )),
        Expression::Instantiation(inst) => {
            let name = expr_to_subject_text(inst.class);
            (!name.is_empty()).then(|| (SubjectText::owned(name), true))
        }
        _ => None,
    }
}

/// Emit navigable parameters embedded in Laravel middleware strings.
///
/// Handles `can:ability,Model` and, when `include_auth_guards` is true,
/// `auth:guard[,guard]`, each accepted as one string or as an array value.
pub(in crate::symbol_map::extraction) fn try_emit_can_middleware_spans(
    argument_list: &ArgumentList<'_>,
    include_auth_guards: bool,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(middleware) = argument_expr_for_parameter(argument_list, "middleware") else {
        return;
    };
    // Laravel reads a middleware array's values whatever their keys.
    let Some(elements) = crate::parser::array_literal_elements(middleware) else {
        push_middleware_parameter_spans(middleware, include_auth_guards, content, spans);
        return;
    };
    for element in elements.iter() {
        let value = match element {
            ArrayElement::KeyValue(element) => element.value,
            ArrayElement::Value(element) => element.value,
            ArrayElement::Variadic(_) | ArrayElement::Missing(_) => continue,
        };
        push_middleware_parameter_spans(value, include_auth_guards, content, spans);
    }
}

fn push_middleware_parameter_spans(
    expr: &Expression<'_>,
    include_auth_guards: bool,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some((text, inner_start, _)) = string_literal_at_range(expr, content) else {
        return;
    };
    let Some((alias, parameters)) = text.split_once(':') else {
        return;
    };
    let parameter_start = inner_start + alias.len() as u32 + 1;
    if alias == "can" {
        let ability = parameters.split(',').next().unwrap_or(parameters);
        push_embedded_string_span(
            crate::symbol_map::LaravelStringKind::GateAbility,
            ability,
            parameter_start,
            spans,
        );
    } else if include_auth_guards
        && crate::symbol_map::laravel_resources::middleware_resource(&text[..alias.len() + 1])
            == Some(crate::symbol_map::LaravelConfigResource::AuthGuard)
    {
        let mut offset = parameter_start;
        for guard in parameters.split(',') {
            push_embedded_string_span(
                crate::symbol_map::LaravelStringKind::ConfigResource(
                    crate::symbol_map::LaravelConfigResource::AuthGuard,
                ),
                guard,
                offset,
                spans,
            );
            offset += guard.len() as u32 + 1;
        }
    }
}

fn push_embedded_string_span(
    kind: crate::symbol_map::LaravelStringKind,
    key: &str,
    start: u32,
    spans: &mut Vec<SymbolSpan>,
) {
    if key.is_empty() {
        return;
    }
    spans.push(SymbolSpan {
        start,
        end: start + key.len() as u32,
        kind: SymbolKind::LaravelStringKey {
            kind,
            key: key.to_string(),
            is_write: false,
            is_optional: false,
        },
    });
}
