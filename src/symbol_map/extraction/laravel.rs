use mago_span::HasSpan;
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use super::*;

/// Namespace prefix for Laravel's container-injection attributes.
pub(super) const LARAVEL_CONTAINER_ATTR_NS: &str = "Illuminate\\Container\\Attributes\\";
/// Short (non-FQN) names of the container-injection attributes.
pub(super) const LARAVEL_CONTAINER_ATTR_NAMES: &[&str] = &[
    "Config",
    "Database",
    "DB",
    "Cache",
    "Log",
    "Storage",
    "Auth",
    "Authenticated",
];

/// Check whether an attribute class name refers to a Laravel container
/// attribute (`Config`, `Database`, `Cache`, `Log`, `Storage`, `Auth`,
/// `Authenticated`).  Returns the corresponding [`LaravelStringKind`] if
/// so — always `Config` since all container attributes resolve to config
/// sub-keys.
///
/// FQN names (containing `\`) are matched directly against
/// `Illuminate\Container\Attributes\*`.  Short names require the file to
/// import from that namespace; the result of that check is cached in
/// `import_cache` to avoid repeated linear scans of the file content.
pub(super) fn resolve_laravel_container_attr(
    class_name: &str,
    import_cache: &mut Option<bool>,
    content: &str,
) -> Option<crate::symbol_map::LaravelStringKind> {
    if class_name.contains('\\') {
        let stripped = class_name.strip_prefix(LARAVEL_CONTAINER_ATTR_NS)?;
        if LARAVEL_CONTAINER_ATTR_NAMES.contains(&stripped) {
            return Some(crate::symbol_map::LaravelStringKind::Config);
        }
        return None;
    }
    if !LARAVEL_CONTAINER_ATTR_NAMES.contains(&class_name) {
        return None;
    }
    let has_import = *import_cache
        .get_or_insert_with(|| content.contains("use Illuminate\\Container\\Attributes\\"));
    if has_import {
        Some(crate::symbol_map::LaravelStringKind::Config)
    } else {
        None
    }
}

/// If the first argument of `argument_list` is a non-empty, non-interpolated
/// string literal, push a [`SymbolKind::LaravelStringKey`] span covering the
/// string content (inside the quotes) onto `spans`.
///
/// Called by the `config()` function-call extractor and the
/// `Config::get()` / `Config::set()` static-call extractor so that
/// find-references and go-to-definition for Laravel config keys can use
/// the pre-built symbol map instead of re-parsing every file on demand.
pub(super) fn try_emit_laravel_string_span(
    kind: crate::symbol_map::LaravelStringKind,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    emit_laravel_string_span(kind, false, argument_list, content, spans);
}

/// Emit the config-key span for a call on the config repository, marking
/// `Config::set('…')` as a write: it declares the key it names, so the key
/// need not exist in any `config/*.php` file.
pub(super) fn try_emit_laravel_config_key_span(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    emit_laravel_string_span(
        crate::symbol_map::LaravelStringKind::Config,
        member_name.eq_ignore_ascii_case("set"),
        argument_list,
        content,
        spans,
    );
}

fn emit_laravel_string_span(
    kind: crate::symbol_map::LaravelStringKind,
    is_write: bool,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Expression::Literal(literal::Literal::String(s)) = first_arg.value() else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let key = &content[inner_start as usize..inner_end as usize];
    if key.is_empty() {
        return;
    }

    if kind == crate::symbol_map::LaravelStringKind::Config && !key.contains('.') {
        // Require at least one dot: bare keys like 'app' are not valid config paths.
        return;
    }

    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelStringKey {
            key: normalised_key(kind.clone(), key),
            kind,
            is_write,
        },
    });
}

/// A view name is stored in its canonical dotted spelling so that the
/// slash-separated form Laravel also accepts compares equal to it; every
/// other kind is stored as written.
fn normalised_key(kind: crate::symbol_map::LaravelStringKind, key: &str) -> String {
    if kind == crate::symbol_map::LaravelStringKind::View {
        crate::virtual_members::laravel::canonical_view_name(key).into_owned()
    } else {
        key.to_string()
    }
}

/// The morph-map API on `Illuminate\Database\Eloquent\Relations\Relation`
/// whose first argument holds the alias → model map.
pub(super) fn is_morph_map_registration_method(member_name: &str) -> bool {
    member_name.eq_ignore_ascii_case("morphMap")
        || member_name.eq_ignore_ascii_case("enforceMorphMap")
}

/// Whether `member_name` is a query-builder method whose second argument is a
/// list of morph types (`whereHasMorph($relation, ['post', 'video'], …)`).
///
/// Laravel maps each entry through `Relation::getMorphedModel()`, so a plain
/// string there is a morph alias.
pub(super) fn is_morph_types_query_method(member_name: &str) -> bool {
    matches!(
        member_name.to_ascii_lowercase().as_str(),
        "hasmorph"
            | "orhasmorph"
            | "doesnthavemorph"
            | "ordoesnthavemorph"
            | "wherehasmorph"
            | "orwherehasmorph"
            | "wheredoesnthavemorph"
            | "orwheredoesnthavemorph"
            | "wheremorphrelation"
            | "orwheremorphrelation"
            | "wheremorphdoesnthaverelation"
            | "orwheremorphdoesnthaverelation"
    )
}

/// Push a [`crate::symbol_map::LaravelStringKind::MorphAlias`] span for every
/// alias key of a `Relation::morphMap(['post' => Post::class, …])` argument.
pub(super) fn try_emit_morph_map_key_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let elements = match first_arg.value() {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return,
    };
    for element in elements.iter() {
        if let ArrayElement::KeyValue(kv) = element {
            push_morph_alias_span(kv.key, content, spans);
        }
    }
}

/// Push a morph-alias span for the `$types` argument at `arg_index`, which
/// Laravel accepts as either a single string or an array of strings.
pub(super) fn try_emit_morph_type_spans(
    argument_list: &ArgumentList<'_>,
    arg_index: usize,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(arg) = argument_list.arguments.iter().nth(arg_index) else {
        return;
    };
    match arg.value() {
        Expression::Array(array) => {
            for element in array.elements.iter() {
                if let ArrayElement::Value(value) = element {
                    push_morph_alias_span(value.value, content, spans);
                }
            }
        }
        Expression::LegacyArray(array) => {
            for element in array.elements.iter() {
                if let ArrayElement::Value(value) = element {
                    push_morph_alias_span(value.value, content, spans);
                }
            }
        }
        expr => push_morph_alias_span(expr, content, spans),
    }
}

/// Push a morph-alias span for a single string-literal expression, skipping the
/// spellings that are not aliases.
fn push_morph_alias_span(expr: &Expression<'_>, content: &str, spans: &mut Vec<SymbolSpan>) {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let alias = &content[inner_start as usize..inner_end as usize];
    // `'*'` is the wildcard the `whereHasMorph()` family accepts, and a value
    // containing a namespace separator is a class name rather than an alias
    // (both APIs take either form).
    if alias.is_empty() || alias == "*" || alias.contains('\\') {
        return;
    }
    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelStringKey {
            kind: crate::symbol_map::LaravelStringKind::MorphAlias,
            key: alias.to_string(),
            is_write: false,
        },
    });
}

// ─── Authorization gate abilities ───────────────────────────────────────────

/// The synthetic function the Blade preprocessor lowers `@can`, `@cannot`,
/// and `@canany` to, so their ability strings reach the same extraction as
/// the PHP forms.
pub(crate) const BLADE_CAN_DIRECTIVE: &str = "blade_can_directive";

/// Methods on Laravel's `Gate` whose first argument is an ability name (or a
/// list of them), and whose second is the model the check is about.
///
/// Compared case-insensitively without lowercasing into a `String`: this runs
/// for every method call in every file.
pub(super) fn is_gate_check_method(member_name: &str) -> bool {
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

/// Whether a static-call subject names Laravel's `Gate` facade.
pub(super) fn is_gate_facade(clean_subject: &str) -> bool {
    clean_subject.eq_ignore_ascii_case("Gate")
        || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Gate")
}

/// How far back down a method chain a facade root is looked for.
///
/// The chains this recognises are short — `Gate::forUser($user)->allows(…)` is
/// one link, `Route::get(…)->name(…)->middleware(…)` a handful. The bound is
/// what keeps the cost linear: several of the method names that start this
/// search (`has`, `any`, `check`) are also ordinary query-builder methods, and
/// an unbounded walk would rescan the whole spine at every link of a long
/// Eloquent chain.
const FACADE_CHAIN_DEPTH: usize = 4;

/// Whether an instance-method chain roots at the `Gate` facade, as
/// `Gate::forUser($user)->allows('update', $post)` does.
pub(super) fn chain_roots_at_gate(expr: &Expression<'_>) -> bool {
    chain_roots_at_facade(expr, FACADE_CHAIN_DEPTH, &|name| is_gate_facade(name))
}

/// Whether an instance-method chain roots at the `Route` facade, as
/// `Route::get(…)->can('update', 'post')` does.
///
/// The `can()` there names an ability, but its second argument is a *route
/// parameter* name rather than a model, so no model subject is recorded.
pub(super) fn chain_roots_at_route_facade(expr: &Expression<'_>) -> bool {
    chain_roots_at_facade(expr, FACADE_CHAIN_DEPTH, &|name| {
        name.eq_ignore_ascii_case("Route")
    })
}

/// Walk at most `depth` links down a method chain looking for a static call
/// whose class satisfies `is_facade`.
fn chain_roots_at_facade(
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

/// Whether a receiver plainly reads as the authenticated user, which is what
/// makes `->can('update', $post)` an authorization check rather than a
/// same-named method on an unrelated object.
///
/// Matches a variable whose name ends in `user` (`$user`, `$authUser`), a
/// property of that shape (`$this->user`), and any `user()` call
/// (`auth()->user()`, `Auth::user()`, `$request->user()`).  Anything else is
/// left alone: `can()` is too ordinary a method name to claim on sight.
pub(super) fn receiver_is_user_like(expr: &Expression<'_>) -> bool {
    fn ends_with_user(name: &str) -> bool {
        let name = name.trim_start_matches('$');
        name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case("user")
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
pub(super) fn try_emit_gate_ability_spans(
    argument_list: &ArgumentList<'_>,
    ability_index: usize,
    model_index: Option<usize>,
    is_definition: bool,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
    gate_subjects: &mut Vec<crate::symbol_map::GateSubject>,
) {
    let Some(ability_arg) = argument_list.arguments.iter().nth(ability_index) else {
        return;
    };

    let first_span = spans.len();
    match ability_arg.value() {
        Expression::Array(array) => {
            for element in array.elements.iter() {
                if let ArrayElement::Value(value) = element {
                    push_gate_ability_span(value.value, is_definition, content, spans);
                }
            }
        }
        Expression::LegacyArray(array) => {
            for element in array.elements.iter() {
                if let ArrayElement::Value(value) = element {
                    push_gate_ability_span(value.value, is_definition, content, spans);
                }
            }
        }
        expr => push_gate_ability_span(expr, is_definition, content, spans),
    }
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
    is_definition: bool,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    // The span check above already rejects an empty literal, so whatever is
    // between the quotes is a name.
    let ability = &content[inner_start as usize..inner_end as usize];
    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelStringKey {
            kind: crate::symbol_map::LaravelStringKind::GateAbility,
            key: ability.to_string(),
            is_write: is_definition,
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
        Expression::Access(Access::ClassConstant(cca))
            if matches!(
                &cca.constant,
                ClassLikeConstantSelector::Identifier(ident)
                    if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
            ) =>
        {
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

/// Emit gate-ability spans for the `can:ability,Model` entries of a
/// `middleware(...)` argument, which Laravel accepts as a single string or an
/// array of them.
pub(super) fn try_emit_can_middleware_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    match first_arg.value() {
        Expression::Array(array) => {
            for element in array.elements.iter() {
                if let ArrayElement::Value(value) = element {
                    push_can_middleware_span(value.value, content, spans);
                }
            }
        }
        Expression::LegacyArray(array) => {
            for element in array.elements.iter() {
                if let ArrayElement::Value(value) = element {
                    push_can_middleware_span(value.value, content, spans);
                }
            }
        }
        expr => push_can_middleware_span(expr, content, spans),
    }
}

/// Push a gate-ability span covering just the ability part of a
/// `'can:update,post'` middleware string.
fn push_can_middleware_span(expr: &Expression<'_>, content: &str, spans: &mut Vec<SymbolSpan>) {
    let Expression::Literal(literal::Literal::String(s)) = expr else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let text = &content[inner_start as usize..inner_end as usize];
    let Some(rest) = text.strip_prefix("can:") else {
        return;
    };
    let ability = rest.split(',').next().unwrap_or(rest);
    if ability.is_empty() {
        return;
    }
    let start = inner_start + "can:".len() as u32;
    spans.push(SymbolSpan {
        start,
        end: start + ability.len() as u32,
        kind: SymbolKind::LaravelStringKey {
            kind: crate::symbol_map::LaravelStringKind::GateAbility,
            key: ability.to_string(),
            is_write: false,
        },
    });
}

/// If the first argument of `argument_list` is a plain, non-empty string
/// literal, push a [`SymbolKind::CommandOwnParam`] span covering the string
/// content.  Used for `$this->argument('user')` / `$this->option('queue')`
/// inside a console command.
pub(super) fn try_emit_command_own_param_span(
    is_option: bool,
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Expression::Literal(literal::Literal::String(s)) = first_arg.value() else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let name = &content[inner_start as usize..inner_end as usize];
    if name.is_empty() {
        return;
    }
    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::CommandOwnParam {
            name: name.to_string(),
            is_option,
        },
    });
}

/// If the first argument of `argument_list` is a non-empty, non-interpolated
/// string literal, push a [`SymbolKind::LaravelStringKey`] span covering the
/// string content (inside the quotes) onto `spans`.
///
/// Called by the `config()` function-call extractor and the
/// `Config::get()` / `Config::set()` static-call extractor so that
/// find-references and go-to-definition for Laravel config keys can use
/// the pre-built symbol map instead of re-parsing every file on demand.
pub(super) fn try_emit_laravel_string_span_partial(
    kind: crate::symbol_map::LaravelStringKind,
    argument_list: &PartialArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Some(Expression::Literal(literal::Literal::String(s))) = first_arg.value() else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let key = &content[inner_start as usize..inner_end as usize];
    if key.is_empty() {
        return;
    }

    if kind == crate::symbol_map::LaravelStringKind::Config && !key.contains('.') {
        // Require at least one dot: bare keys like 'app' are not valid config paths.
        return;
    }

    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelStringKey {
            key: normalised_key(kind.clone(), key),
            kind,
            is_write: false,
        },
    });
}

/// If `argument_list` starts with a plain, non-empty string literal, push a
/// [`SymbolKind::LaravelMacroString`] span covering the string content.
pub(super) fn try_emit_laravel_macro_string_span(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    let Some(first_arg) = argument_list.arguments.iter().next() else {
        return;
    };
    let Expression::Literal(literal::Literal::String(s)) = first_arg.value() else {
        return;
    };
    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let name = &content[inner_start as usize..inner_end as usize];
    if name.is_empty() {
        return;
    }
    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::LaravelMacroString {
            name: name.to_string(),
        },
    });
}

/// Detect an array-callable literal — `[Class::class, 'method']` or
/// `[$object, 'method']` — and emit a [`SymbolKind::MemberAccess`] span over
/// the method-name string so that go-to-definition, references, and rename
/// treat it like a real `Class::method` / `$object->method` reference.
///
/// This is the shape Laravel routes use for controller actions
/// (`Route::get('/', [IndexPageController::class, 'indexPage'])`), but the
/// pattern is generic PHP (event listeners, `array_map`, etc.).
///
/// Only fires for a two-element array whose first element is a `::class`
/// constant or a variable and whose second element is a plain string literal.
pub(super) fn try_emit_array_callable_span(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    // Exactly two positional (value) elements: `[<callee>, '<method>']`.
    let mut values = elements.iter().filter_map(|el| match el {
        ArrayElement::Value(v) => Some(v.value),
        _ => None,
    });
    let (Some(first), Some(second), None) = (values.next(), values.next(), values.next()) else {
        return;
    };
    // The whole array must have been exactly those two elements (no
    // key/value or variadic entries mixed in).
    if elements.iter().count() != 2 {
        return;
    }

    // Second element must be a plain string literal — the method name.
    let Expression::Literal(literal::Literal::String(s)) = second else {
        return;
    };

    // First element determines the subject and access kind.
    let (subject_text, subject_span, is_static) = match first {
        // `Class::class` — static-style access on the class name.
        Expression::Access(Access::ClassConstant(cca)) => {
            let is_class_const = matches!(
                &cca.constant,
                ClassLikeConstantSelector::Identifier(ident)
                    if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
            );
            if !is_class_const {
                return;
            }
            (expr_to_subject_text(cca.class), cca.class.span(), true)
        }
        // `$this` / `$object` — instance-style access.
        Expression::Variable(Variable::Direct(dv)) => {
            (bytes_to_str(dv.name).to_string(), dv.span, false)
        }
        _ => return,
    };
    if subject_text.is_empty() {
        return;
    }

    let inner_start = s.span.start.offset + 1;
    let inner_end = s.span.end.offset - 1;
    if inner_start >= inner_end || inner_end as usize > content.len() {
        return;
    }
    let member_name = if let Some(value) = s.value {
        bytes_to_str(value)
    } else {
        &content[inner_start as usize..inner_end as usize]
    };
    // Guard against non-identifier strings (e.g. `[$a, 'some text']`).
    if member_name.is_empty()
        || !member_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        || member_name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    {
        return;
    }

    spans.push(SymbolSpan {
        start: inner_start,
        end: inner_end,
        kind: SymbolKind::MemberAccess {
            subject_text: SubjectText::new(
                subject_text,
                subject_span.start.offset,
                subject_span.end.offset,
                content,
            ),
            member_name: crate::atom::atom(member_name),
            is_static,
            is_method_call: true,
            is_docblock_reference: false,
            is_array_callable: true,
        },
    });
}

/// If `argument_list` belongs to a `compact()` call, emit one
/// [`SymbolKind::CompactVariable`] span per string-literal variable name,
/// including names inside (possibly nested) array-literal arguments.
pub(super) fn try_emit_compact_string_spans(
    argument_list: &ArgumentList<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    for arg in argument_list.arguments.iter() {
        emit_compact_name_spans(arg.value(), content, spans);
    }
}

/// Emit [`SymbolKind::CompactVariable`] spans for one `compact()` argument:
/// a string literal names a variable directly, and an array literal is
/// descended into recursively so `compact(['a', ['b']])` covers both names.
pub(super) fn emit_compact_name_spans(
    expr: &Expression<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match expr {
        Expression::Literal(literal::Literal::String(s)) => {
            let inner_start = s.span.start.offset + 1;
            let inner_end = s.span.end.offset - 1;
            if inner_start >= inner_end || inner_end as usize > content.len() {
                return;
            }

            let name = if let Some(value) = s.value {
                bytes_to_str(value)
            } else {
                &content[inner_start as usize..inner_end as usize]
            };
            if name.is_empty() {
                return;
            }

            spans.push(SymbolSpan {
                start: inner_start,
                end: inner_end,
                kind: SymbolKind::CompactVariable {
                    name: crate::atom::atom(name),
                },
            });
        }
        Expression::Array(arr) => {
            for elem in arr.elements.iter() {
                emit_compact_name_elem_spans(elem, content, spans);
            }
        }
        Expression::LegacyArray(arr) => {
            for elem in arr.elements.iter() {
                emit_compact_name_elem_spans(elem, content, spans);
            }
        }
        _ => {}
    }
}

/// Emit spans for one element of an array passed to `compact()`. Keys are
/// ignored; values are variable names or nested arrays.
pub(super) fn emit_compact_name_elem_spans(
    elem: &ArrayElement<'_>,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match elem {
        ArrayElement::KeyValue(kv) => emit_compact_name_spans(kv.value, content, spans),
        ArrayElement::Value(v) => emit_compact_name_spans(v.value, content, spans),
        ArrayElement::Variadic(s) => emit_compact_name_spans(s.value, content, spans),
        ArrayElement::Missing(_) => {}
    }
}

/// Returns `true` if `name` is a method on Laravel's `Repository` config contract
/// that accepts a config key as its first argument.
pub(super) fn is_config_repository_method(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "has"
            | "get"
            | "string"
            | "integer"
            | "float"
            | "boolean"
            | "array"
            | "collection"
            | "set"
            | "prepend"
            | "push"
    )
}

/// Returns `true` for a static call whose first argument is an Artisan
/// command name: `Artisan::call(...)`, `Artisan::queue(...)`, or
/// `Schedule::command(...)`.  Recognises both the short facade name and its
/// `Illuminate\Support\Facades\` FQN.
pub(super) fn is_artisan_command_static_call(clean_subject: &str, member_name: &str) -> bool {
    let is_artisan = clean_subject.eq_ignore_ascii_case("Artisan")
        || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Artisan");
    let is_schedule = clean_subject.eq_ignore_ascii_case("Schedule")
        || clean_subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\Schedule");
    let member = member_name.to_ascii_lowercase();
    (is_artisan && matches!(member.as_str(), "call" | "queue"))
        || (is_schedule && member == "command")
}

/// Returns `true` if `object` is a `config()` (or `\config()`) helper call and
/// `member_name` is a config-key-accepting method, e.g. `config()->get('app.name')`.
pub(super) fn is_laravel_config_repository_call(
    object: &Expression<'_>,
    member_name: &str,
) -> bool {
    if !is_config_repository_method(member_name) {
        return false;
    }

    match object {
        Expression::Call(Call::Function(func_call)) => match func_call.function {
            Expression::Identifier(ident) => {
                strip_fqn_prefix(bytes_to_str(ident.value())).eq_ignore_ascii_case("config")
            }
            _ => false,
        },
        _ => false,
    }
}

// ─── Laravel route controller method spans ──────────────────────────────────

/// HTTP method names used in Laravel route definitions.
///
/// `Route::get(…)`, `Route::post(…)`, etc.  `match` is excluded because
/// its action argument is in a different position (3rd, not 2nd).
pub(super) const ROUTE_HTTP_METHODS: &[&str] =
    &["get", "post", "put", "patch", "delete", "options", "any"];

/// Walk a fluent method chain backwards looking for `->controller(X::class)`.
///
/// Returns the class name (as written in source, e.g. `"WorkItemController"`)
/// if found, `None` otherwise.  Handles both instance-method chains
/// (`->prefix('…')->controller(X::class)`) and a static entry point
/// (`Route::controller(X::class)`).
pub(super) fn laravel_route_find_controller_in_chain(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Call(Call::Method(mc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method
                && ident.value.eq_ignore_ascii_case(b"controller")
            {
                return laravel_route_extract_class_arg(&mc.argument_list);
            }
            laravel_route_find_controller_in_chain(mc.object)
        }
        Expression::Call(Call::StaticMethod(sc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &sc.method
                && ident.value.eq_ignore_ascii_case(b"controller")
            {
                return laravel_route_extract_class_arg(&sc.argument_list);
            }
            None
        }
        _ => None,
    }
}

/// Extract the class name from the first argument when it is a `X::class`
/// constant access.  Returns the source text of `X` (e.g.
/// `"WorkItemResourceController"` or `"App\\Http\\Controllers\\Foo"`).
pub(super) fn laravel_route_extract_class_arg(args: &ArgumentList<'_>) -> Option<String> {
    let first_arg = args.arguments.iter().next()?;
    if let Expression::Access(Access::ClassConstant(cca)) = first_arg.value() {
        let is_class = matches!(
            &cca.constant,
            ClassLikeConstantSelector::Identifier(ident)
                if bytes_to_str(ident.value).eq_ignore_ascii_case("class")
        );
        if is_class {
            let name = expr_to_subject_text(cca.class);
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Walk the closure (or arrow function) body passed to `->group()`.
pub(super) fn laravel_route_scan_group_body(
    expr: &Expression<'_>,
    controller: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match expr {
        Expression::Closure(closure) => {
            for stmt in closure.body.statements.iter() {
                laravel_route_scan_stmt(stmt, controller, content, spans);
            }
        }
        Expression::ArrowFunction(af) => {
            laravel_route_scan_expr(af.expression, controller, content, spans);
        }
        _ => {}
    }
}

/// Scan a statement for route definition calls with controller method strings.
pub(super) fn laravel_route_scan_stmt(
    stmt: &Statement<'_>,
    controller: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match stmt {
        Statement::Expression(e) => {
            laravel_route_scan_expr(e.expression, controller, content, spans);
        }
        Statement::Return(r) => {
            if let Some(v) = r.value {
                laravel_route_scan_expr(v, controller, content, spans);
            }
        }
        Statement::If(if_stmt) => {
            for s in if_stmt.body.statements() {
                laravel_route_scan_stmt(s, controller, content, spans);
            }
            for stmts in if_stmt.body.else_if_statements() {
                for s in stmts {
                    laravel_route_scan_stmt(s, controller, content, spans);
                }
            }
            if let Some(else_stmts) = if_stmt.body.else_statements() {
                for s in else_stmts {
                    laravel_route_scan_stmt(s, controller, content, spans);
                }
            }
        }
        Statement::Foreach(fe) => {
            for s in fe.body.statements() {
                laravel_route_scan_stmt(s, controller, content, spans);
            }
        }
        _ => {}
    }
}

/// Scan an expression for `Route::get(…)` / `Route::post(…)` / etc. and
/// emit a [`SymbolKind::MemberAccess`] span for the action string.
///
/// Also recurses into nested `->group()` calls that do **not** declare
/// their own `->controller()` (inheriting the parent controller), while
/// stopping at nested groups that **do** declare a new controller (those
/// are handled by their own extraction-time invocation).
pub(super) fn laravel_route_scan_expr(
    expr: &Expression<'_>,
    controller: &str,
    content: &str,
    spans: &mut Vec<SymbolSpan>,
) {
    match expr {
        // Chained call: Route::patch('cancel', 'cancel')->name('cancel')
        Expression::Call(Call::Method(mc)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &mc.method
                && ident.value.eq_ignore_ascii_case(b"group")
            {
                // Nested group — check for its own controller.
                if laravel_route_find_controller_in_chain(mc.object).is_some() {
                    return; // Own controller; handled separately.
                }
                // No new controller — inherit the parent's.
                for arg in mc.argument_list.arguments.iter() {
                    laravel_route_scan_group_body(arg.value(), controller, content, spans);
                }
                return;
            }
            // Walk the inner object (chain before ->name() etc.).
            laravel_route_scan_expr(mc.object, controller, content, spans);
        }
        // Route::patch('cancel', 'cancel')
        Expression::Call(Call::StaticMethod(sc)) => {
            let subject = expr_to_subject_text(sc.class);
            if !strip_fqn_prefix(&subject).eq_ignore_ascii_case("Route") {
                return;
            }
            let ClassLikeMemberSelector::Identifier(ident) = &sc.method else {
                return;
            };
            let method_lower = bytes_to_str(ident.value).to_ascii_lowercase();
            if !ROUTE_HTTP_METHODS.iter().any(|m| *m == method_lower) {
                return;
            }
            // Second argument is the controller method name.
            let mut args_iter = sc.argument_list.arguments.iter();
            let _uri_arg = args_iter.next(); // skip first (URI)
            let Some(action_arg) = args_iter.next() else {
                return;
            };
            let Expression::Literal(literal::Literal::String(s)) = action_arg.value() else {
                return;
            };

            let inner_start = s.span.start.offset + 1;
            let inner_end = s.span.end.offset - 1;
            if inner_start >= inner_end || inner_end as usize > content.len() {
                return;
            }
            let method_name = if let Some(value) = s.value {
                bytes_to_str(value)
            } else {
                &content[inner_start as usize..inner_end as usize]
            };
            // Must look like a valid PHP method name.
            if method_name.is_empty()
                || !method_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                || method_name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            {
                return;
            }

            spans.push(SymbolSpan {
                start: inner_start,
                end: inner_end,
                kind: SymbolKind::MemberAccess {
                    subject_text: SubjectText::owned(controller.to_string()),
                    member_name: crate::atom::atom(method_name),
                    is_static: true,
                    is_method_call: true,
                    is_docblock_reference: false,
                    is_array_callable: false,
                },
            });
        }
        _ => {}
    }
}
