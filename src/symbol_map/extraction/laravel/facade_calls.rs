//! The names static calls on Laravel's facades and its helper functions
//! take in their first argument: `Config::get('app.name')`,
//! `Lang::get('messages.saved')`, `Artisan::call('app:sync')`,
//! `route('home')`, `env('APP_KEY')` and the rest.
//!
//! The symbol map emits its spans through the per-family predicates here,
//! and completion classifies the call it is typed into through
//! [`facade_static_call_arg_kind`] and [`helper_function_first_arg_kind`],
//! so the two cannot disagree about which calls name what.

use super::*;
use crate::blade::markers;
use crate::symbol_map::StringKeyKind;

/// Whether a static call's class, as written, is `Illuminate\Support\Env`.
pub(in crate::symbol_map::extraction) fn is_env_class_name(name: &str) -> bool {
    matches_laravel_class(name, "Illuminate\\Support", "Env")
}

/// Whether a static call reads an environment variable the way the `env()`
/// helper it backs does: `Env::get('APP_NAME')`.
pub(in crate::symbol_map::extraction) fn is_env_key_call(
    clean_subject: &str,
    member_name: &str,
) -> bool {
    is_env_class_name(clean_subject)
        && (member_name.eq_ignore_ascii_case("get")
            || member_name.eq_ignore_ascii_case("getOrFail"))
}

/// Whether a static call on the `Lang` facade names a translation key.
pub(in crate::symbol_map::extraction) fn is_lang_key_call(
    clean_subject: &str,
    member_name: &str,
) -> bool {
    matches_laravel_facade(clean_subject, "Lang")
        && ["get", "has", "hasForLocale", "choice"]
            .iter()
            .any(|name| member_name.eq_ignore_ascii_case(name))
}

/// Whether a static call's class, as written, is Eloquent's `Relation`.
pub(in crate::symbol_map::extraction) fn is_relation_class_name(name: &str) -> bool {
    matches_laravel_class(
        name,
        "Illuminate\\Database\\Eloquent\\Relations",
        "Relation",
    )
}

/// Whether a static call looks a morph alias up:
/// `Relation::getMorphedModel('post')`, or
/// `Post::getActualClassNameForMorph('post')` on any model.
pub(in crate::symbol_map::extraction) fn is_morph_alias_lookup_call(
    clean_subject: &str,
    member_name: &str,
) -> bool {
    member_name.eq_ignore_ascii_case("getActualClassNameForMorph")
        || (is_relation_class_name(clean_subject)
            && member_name.eq_ignore_ascii_case("getMorphedModel"))
}

/// Whether a static call names a template in its first argument: a view
/// factory method on the `View` facade, or `Response::view()`.
pub(in crate::symbol_map::extraction) fn is_view_first_arg_call(
    clean_subject: &str,
    member_name: &str,
) -> bool {
    (matches_laravel_facade(clean_subject, "View")
        && view_factory_method_name_index(member_name) == Some(0))
        || (matches_laravel_facade(clean_subject, "Response")
            && member_name.eq_ignore_ascii_case("view"))
}

/// The argument `Route::view('/about', 'pages.about')` names its template
/// in, after the URI it binds.
const ROUTE_VIEW_ARG: usize = 1;

/// Whether a static call binds a URI straight to a template:
/// `Route::view('/about', 'pages.about', [...])`.
fn is_route_view_call(clean_subject: &str, member_name: &str) -> bool {
    matches_laravel_facade(clean_subject, "Route") && member_name.eq_ignore_ascii_case("view")
}

/// Whether a static call checks the current route against the names it
/// lists in every argument: `Route::is('admin.*')`,
/// `Route::currentRouteNamed(…)`.
fn is_route_pattern_call(clean_subject: &str, member_name: &str) -> bool {
    matches_laravel_facade(clean_subject, "Route") && is_route_facade_pattern_method(member_name)
}

/// The kind of name a static Laravel call takes in its positional argument
/// `index`, by the class as written (without a leading `\`) and the method.
pub(crate) fn facade_static_call_arg_kind(
    clean_subject: &str,
    member_name: &str,
    index: usize,
) -> Option<StringKeyKind> {
    if index == 0
        && let Some(kind) = facade_static_call_first_arg_kind(clean_subject, member_name)
    {
        return Some(kind);
    }
    if (matches_laravel_facade(clean_subject, "View")
        && view_factory_names_view_at(member_name, index))
        || (index == ROUTE_VIEW_ARG && is_route_view_call(clean_subject, member_name))
    {
        Some(StringKeyKind::View)
    } else if is_route_pattern_call(clean_subject, member_name) {
        Some(StringKeyKind::Route)
    } else if matches_laravel_facade(clean_subject, "App")
        && container_key_argument(member_name).is_some_and(|(arg, _)| arg == index)
    {
        Some(StringKeyKind::ContainerBinding)
    } else if index == MORPH_TYPES_ARG && is_morph_types_query_method(member_name) {
        Some(StringKeyKind::MorphAlias)
    } else {
        None
    }
}

/// Whether an argument that names a `kind` may list several in an array
/// literal as well as name one, for a static or an instance call of
/// `member_name`: an ability check (`Gate::any([…])`), the morph types a
/// `whereHasMorph()` takes, a view factory's candidates (`View::first([…])`)
/// and `Config::getMany([…])`.  The list's bare entries are the names; a
/// value under a `'key' =>` is not one.
pub(crate) fn argument_takes_name_list(kind: &StringKeyKind, member_name: &str) -> bool {
    match kind {
        StringKeyKind::GateAbility | StringKeyKind::MorphAlias => true,
        StringKeyKind::View => view_factory_method_name_index(member_name).is_some(),
        StringKeyKind::Config => member_name.eq_ignore_ascii_case("getMany"),
        _ => false,
    }
}

/// [`facade_static_call_arg_kind`] for the first argument, which most of
/// these calls name what they name in.
fn facade_static_call_first_arg_kind(
    clean_subject: &str,
    member_name: &str,
) -> Option<StringKeyKind> {
    if is_config_facade_name(clean_subject) && is_config_repository_method(member_name) {
        Some(StringKeyKind::Config)
    } else if is_view_first_arg_call(clean_subject, member_name) {
        Some(StringKeyKind::View)
    } else if is_route_name_facade_call(clean_subject, member_name)
        || is_route_pattern_call(clean_subject, member_name)
    {
        Some(StringKeyKind::Route)
    } else if is_env_key_call(clean_subject, member_name) {
        Some(StringKeyKind::Env)
    } else if is_lang_key_call(clean_subject, member_name) {
        Some(StringKeyKind::Trans)
    } else if is_artisan_command_static_call(clean_subject, member_name) {
        Some(StringKeyKind::Command)
    } else if is_gate_facade(clean_subject)
        && (member_name.eq_ignore_ascii_case("define") || is_gate_check_method(member_name))
    {
        Some(StringKeyKind::GateAbility)
    } else if is_morph_alias_lookup_call(clean_subject, member_name) {
        Some(StringKeyKind::MorphAlias)
    } else {
        None
    }
}

/// Emit the Laravel string spans a static call carries, keyed on the
/// facade (or class) it is written against.
///
/// `clean_subject` is the class as written and `semantic_subject` the name
/// it resolves to. When that is a bare global name Laravel only provides as
/// a runtime facade alias, the spans are deferred until the alias is known
/// to reach the facade.
pub(in crate::symbol_map::extraction) fn emit_laravel_static_call_spans<'a>(
    clean_subject: &str,
    semantic_subject: &str,
    member_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    let laravel_span_start = ctx.spans.len();
    if is_config_facade_name(clean_subject) && is_config_repository_method(member_name) {
        try_emit_laravel_config_key_span(member_name, argument_list, ctx.content, &mut ctx.spans);
    }
    if let Some(trigger) =
        crate::symbol_map::laravel_resources::static_method_trigger(semantic_subject, member_name)
    {
        try_emit_laravel_config_resource_span_for_parameter(
            trigger.kind,
            trigger.shape,
            trigger.access,
            argument_list,
            trigger.argument,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // The `View` facade proxies the view factory, so every
    // factory method that names a template does so here too.
    if matches_laravel_facade(clean_subject, "View") {
        if let Some(index) = view_factory_method_name_index(member_name) {
            try_emit_laravel_view_span_at(index, argument_list, ctx.content, &mut ctx.spans);
        } else if is_render_each_method(member_name) {
            try_emit_render_each_view_spans(argument_list, ctx.content, &mut ctx.spans);
        }
    }
    // `Route::view('/about', 'pages.about', [...])` binds a URI
    // straight to a template, naming the view second;
    // `Response::view('pages.about', [...])` names it first.
    if is_route_view_call(clean_subject, member_name) {
        try_emit_laravel_string_span_at(
            crate::symbol_map::StringKeyKind::View,
            ROUTE_VIEW_ARG,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    if matches_laravel_facade(clean_subject, "Response") && member_name.eq_ignore_ascii_case("view")
    {
        try_emit_laravel_string_span(
            crate::symbol_map::StringKeyKind::View,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // `URL::signedRoute('orders.show')`,
    // `Redirect::route('home')`, `Response::redirectToRoute(…)`.
    if is_route_name_facade_call(clean_subject, member_name) {
        try_emit_laravel_string_span(
            crate::symbol_map::StringKeyKind::Route,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // `Route::is('admin.*')` / `Route::currentRouteNamed(…)` list
    // the names the current route is checked against.
    if is_route_pattern_call(clean_subject, member_name) {
        try_emit_laravel_string_spans_all(
            crate::symbol_map::StringKeyKind::Route,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // `Env::get('APP_NAME')` reads the same variable the `env()`
    // helper it backs does.
    if is_env_key_call(clean_subject, member_name) {
        try_emit_laravel_string_span(
            crate::symbol_map::StringKeyKind::Env,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    if is_lang_key_call(clean_subject, member_name) {
        try_emit_laravel_string_span(
            crate::symbol_map::StringKeyKind::Trans,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // `App::make('payments')` and the registrations written
    // against the same facade.
    if matches_laravel_facade(clean_subject, "App") {
        try_emit_container_key_span(member_name, argument_list, ctx.content, &mut ctx.spans);
    }
    // Artisan command names: `Artisan::call('app:sync')`,
    // `Artisan::queue('app:sync')`, `Schedule::command('app:sync')`.
    if is_artisan_command_static_call(clean_subject, member_name) {
        try_emit_laravel_string_span(
            crate::symbol_map::StringKeyKind::Command,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // Authorization abilities checked (or defined) through the
    // `Gate` facade, and the `can:ability,Model` middleware
    // parameter of a route registration.
    if is_gate_facade(clean_subject) {
        emit_gate_facade_ability_spans(member_name, argument_list, ctx);
    } else if matches_laravel_facade(semantic_subject, "Route")
        && member_name.eq_ignore_ascii_case("middleware")
    {
        try_emit_can_middleware_spans(argument_list, true, ctx.content, &mut ctx.spans);
    }
    // Eloquent morph aliases: the keys of a
    // `Relation::morphMap(['post' => Post::class])` registration,
    // and the argument of `Relation::getMorphedModel('post')` /
    // `Model::getActualClassNameForMorph('post')`.
    if is_relation_class_name(clean_subject) && is_morph_map_registration_method(member_name) {
        try_emit_morph_map_key_spans(argument_list, ctx.content, &mut ctx.spans);
    } else if is_morph_alias_lookup_call(clean_subject, member_name) {
        try_emit_morph_type_spans(argument_list, 0, ctx.content, &mut ctx.spans);
    }
    // A model forwards static calls to its query builder, so
    // `Comment::whereHasMorph('commentable', ['post'])` is the same
    // call site as the instance form.
    if is_morph_types_query_method(member_name) {
        try_emit_morph_type_spans(argument_list, MORPH_TYPES_ARG, ctx.content, &mut ctx.spans);
    }
    if let Some(dependency) = ctx
        .resolved_names
        .and_then(|_| crate::symbol_map::StringKeyDependency::root_facade(semantic_subject))
    {
        ctx.defer_string_key_spans_since(laravel_span_start, dependency);
    }
}

// ─── Authorization gate abilities ───────────────────────────────────────────

/// Emit ability spans for a `Gate::<method>(…)` static call.
///
/// `define()` declares the ability it names, so its span is marked as a write
/// and never judged against the known set.  Every other gate method reads one
/// (or a list), with the model — when the call names one — in the second
/// argument.
fn emit_gate_facade_ability_spans<'a>(
    member_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    if member_name.eq_ignore_ascii_case("define") {
        try_emit_gate_ability_spans(
            argument_list,
            0,
            None,
            AbilityRole::Definition,
            ctx.content,
            &mut ctx.spans,
            &mut ctx.gate_subjects,
        );
        return;
    }
    if is_gate_check_method(member_name) {
        try_emit_gate_ability_spans(
            argument_list,
            0,
            Some(1),
            gate_ability_role(member_name),
            ctx.content,
            &mut ctx.spans,
            &mut ctx.gate_subjects,
        );
    }
}

/// The marker functions the Blade preprocessor lowers the `@include` and
/// `@extends` families, `@component`/`@componentFirst` and `@each` to.
const BLADE_VIEW_MARKERS: [&str; 3] = [markers::VIEW, markers::COMPONENT, markers::EACH];

/// The kind of name a Laravel helper function takes in its first argument,
/// by the function's name as written (without a leading `\`).
///
/// A Blade directive that names a template or an ability is lowered to a
/// marker call, so the marker names what the directive does.
pub(crate) fn helper_function_first_arg_kind(name: &str) -> Option<StringKeyKind> {
    let is_one_of = |names: &[&str]| names.iter().any(|n| name.eq_ignore_ascii_case(n));
    if name.eq_ignore_ascii_case("view") || is_one_of(&BLADE_VIEW_MARKERS) {
        Some(StringKeyKind::View)
    } else if is_one_of(&["route", "to_route"]) {
        Some(StringKeyKind::Route)
    } else if is_one_of(&["__", "trans", "trans_choice"]) {
        Some(StringKeyKind::Trans)
    } else if name.eq_ignore_ascii_case("config") {
        Some(StringKeyKind::Config)
    } else if name.eq_ignore_ascii_case("env") {
        Some(StringKeyKind::Env)
    } else if is_one_of(&["app", "resolve"]) {
        Some(StringKeyKind::ContainerBinding)
    } else if name.eq_ignore_ascii_case(markers::CAN) {
        Some(StringKeyKind::GateAbility)
    } else {
        None
    }
}

/// Whether a helper takes a list of names in its first argument as well as
/// a single one: `@includeFirst`, `@componentFirst` and `@extendsFirst`
/// lower to their directive's marker with an array of candidate templates,
/// and `@canany` to the `@can` marker with an array of abilities.
pub(crate) fn helper_function_accepts_name_list(name: &str) -> bool {
    [markers::VIEW, markers::COMPONENT, markers::CAN]
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}
