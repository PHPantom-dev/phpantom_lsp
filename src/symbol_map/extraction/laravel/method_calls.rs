//! The names instance method calls take in their arguments:
//! `redirect()->route('home')`, `$user->can('update', $post)`,
//! `$this->call('app:sync')`, `view()->renderWhen($ok, 'partials.flash')`
//! and the rest.
//!
//! Most of these method names are too common to claim on sight, so whether
//! a call names anything depends on what its receiver reads as.  The symbol
//! map reads that off the AST ([`AstReceiver`]) and completion off the text
//! before the call; each answers the [`MethodReceiver`] questions its own
//! way, and both go through the per-family predicates here, so the two
//! cannot disagree about which calls name what.

use super::*;
use crate::symbol_map::StringKeyKind;

/// What an instance method call's receiver reads as.
///
/// Each predicate checks the method name before it asks: the symbol map
/// classifies every method call in every file, and most of these facts
/// walk the receiver's chain.
pub(crate) trait MethodReceiver {
    /// `$this`.
    fn is_this(&self) -> bool;
    /// `$this` inside an Artisan console command.
    fn is_console_command(&self) -> bool;
    /// `$this` inside a mailable.
    fn is_mailable(&self) -> bool;
    /// Plainly the authenticated user: `$user`, `$this->user`,
    /// `auth()->user()`.
    fn is_user_like(&self) -> bool;
    /// A chain rooted at the `Gate` facade: `Gate::forUser($user)->…`.
    fn roots_at_gate(&self) -> bool;
    /// A chain rooted at the `Route` facade: a route registration.
    fn roots_at_route(&self) -> bool;
    /// A bare `redirect()`, `url()` or `response()` call, whose result
    /// builds URLs from route names.
    fn is_route_name_helper(&self) -> bool;
    /// A bare `view()` call: the view factory.
    fn is_view_factory(&self) -> bool;
    /// A `config()` call: the config repository.
    fn is_config_repository(&self) -> bool;
    /// The service container: `app()`, `$app`, or `$this->app`.
    fn is_container(&self) -> bool;
}

/// The receiver of an instance method call, read off the AST.
pub(in crate::symbol_map::extraction) struct AstReceiver<'r> {
    pub(in crate::symbol_map::extraction) object: &'r Expression<'r>,
    /// The call sits in a console command's class body.
    pub(in crate::symbol_map::extraction) in_console_command: bool,
    /// The call sits in a mailable's class body.
    pub(in crate::symbol_map::extraction) in_mailable: bool,
    pub(in crate::symbol_map::extraction) resolved_names:
        Option<&'r crate::names::OwnedResolvedNames>,
}

impl MethodReceiver for AstReceiver<'_> {
    fn is_this(&self) -> bool {
        matches!(self.object, Expression::Variable(Variable::Direct(v)) if v.name == b"$this")
    }

    fn is_console_command(&self) -> bool {
        self.in_console_command && self.is_this()
    }

    fn is_mailable(&self) -> bool {
        self.in_mailable && self.is_this()
    }

    fn is_user_like(&self) -> bool {
        receiver_is_user_like(self.object)
    }

    fn roots_at_gate(&self) -> bool {
        chain_roots_at_gate(self.object)
    }

    fn roots_at_route(&self) -> bool {
        chain_roots_at_route_facade(self.object, self.resolved_names)
    }

    fn is_route_name_helper(&self) -> bool {
        is_route_name_helper_receiver(self.object)
    }

    fn is_view_factory(&self) -> bool {
        is_laravel_view_factory_call(self.object)
    }

    fn is_config_repository(&self) -> bool {
        is_config_helper_call(self.object)
    }

    fn is_container(&self) -> bool {
        is_laravel_container_expr(self.object)
    }
}

/// Whether `member_name` reads or writes the config key in its first
/// argument: `config()->get('app.name')`.
pub(in crate::symbol_map::extraction) fn names_config_key(
    member_name: &str,
    receiver: &dyn MethodReceiver,
) -> bool {
    is_config_repository_method(member_name) && receiver.is_config_repository()
}

/// The argument a container method names its binding key in:
/// `$this->app->singleton('payments', …)`, `app()->make('payments')`.
fn container_key_index(member_name: &str, receiver: &dyn MethodReceiver) -> Option<usize> {
    let (index, _) = container_key_argument(member_name)?;
    receiver.is_container().then_some(index)
}

/// Whether `member_name` names a route in its first argument:
/// `redirect()->route('users.index')`, `url()->signedRoute(…)`,
/// `response()->redirectToRoute(…)`.  `route()` is a plain enough method
/// name that only the helper it hangs off says it names one.
pub(in crate::symbol_map::extraction) fn names_route(
    member_name: &str,
    receiver: &dyn MethodReceiver,
) -> bool {
    is_route_name_method(member_name) && receiver.is_route_name_helper()
}

/// Whether `member_name` is a mailable building its content from a
/// template: `$this->view('emails.shipped')`.
pub(in crate::symbol_map::extraction) fn names_mailable_view(
    member_name: &str,
    receiver: &dyn MethodReceiver,
) -> bool {
    is_mailable_view_method(member_name) && receiver.is_mailable()
}

/// Whether `member_name` runs another Artisan command from inside one:
/// `$this->call('app:sync')`.  `->call()` is an extremely common method
/// name elsewhere (`$this->call('GET', '/uri')` in HTTP tests), so only a
/// console command's own `$this` counts.
pub(in crate::symbol_map::extraction) fn names_command(
    member_name: &str,
    receiver: &dyn MethodReceiver,
) -> bool {
    (member_name.eq_ignore_ascii_case("call") || member_name.eq_ignore_ascii_case("callSilently"))
        && receiver.is_console_command()
}

/// Whether `member_name` is one of the `can()` family.
fn is_can_method(member_name: &str) -> bool {
    member_name.eq_ignore_ascii_case("can")
        || member_name.eq_ignore_ascii_case("cannot")
        || member_name.eq_ignore_ascii_case("canAny")
}

/// Where an authorization check written as an instance method call names
/// its ability, and the model it checks the ability against.
#[derive(Clone, Copy)]
pub(crate) struct AbilityArguments {
    pub(crate) ability: usize,
    /// `None` for a route registration's `->can('update', 'post')`, whose
    /// second argument names a route parameter rather than a model.
    pub(crate) model: Option<usize>,
}

/// The authorization check `member_name` makes, if it makes one.
///
/// `->can()` and friends are named too plainly to claim on the method name
/// alone, so each shape is gated on what its receiver reads as: a chain
/// rooted at the `Gate` facade, `$this->authorize()` /
/// `$this->authorizeForUser()` from the `AuthorizesRequests` trait,
/// `->can()` on something that plainly is the authenticated user, and a
/// route registration's `->can()`.
pub(crate) fn ability_arguments(
    member_name: &str,
    receiver: &dyn MethodReceiver,
) -> Option<AbilityArguments> {
    const CHECK: AbilityArguments = AbilityArguments {
        ability: 0,
        model: Some(1),
    };
    if member_name.eq_ignore_ascii_case("authorizeForUser") {
        // The user comes first, shifting the ability and model along by
        // one.  It is a method of the `AuthorizesRequests` trait, so
        // `$this` is the only receiver it has.
        return receiver.is_this().then_some(AbilityArguments {
            ability: 1,
            model: Some(2),
        });
    }
    if is_can_method(member_name) {
        if receiver.roots_at_route() {
            return Some(AbilityArguments {
                ability: 0,
                model: None,
            });
        }
        return receiver.is_user_like().then_some(CHECK);
    }
    if member_name.eq_ignore_ascii_case("authorize") {
        return (receiver.is_this() || receiver.roots_at_gate()).then_some(CHECK);
    }
    (is_gate_check_method(member_name) && receiver.roots_at_gate()).then_some(CHECK)
}

/// The kind of name an instance method call takes in its positional
/// argument `index`.
pub(crate) fn instance_method_arg_kind(
    member_name: &str,
    index: usize,
    receiver: &dyn MethodReceiver,
) -> Option<StringKeyKind> {
    let first = index == 0;
    if first && names_config_key(member_name, receiver) {
        Some(StringKeyKind::Config)
    } else if container_key_index(member_name, receiver) == Some(index) {
        Some(StringKeyKind::ContainerBinding)
    } else if (first && names_route(member_name, receiver))
        // `$request->routeIs('admin.*', 'account.*')`: every argument.
        || is_request_route_pattern_method(member_name)
    {
        Some(StringKeyKind::Route)
    } else if (first && names_mailable_view(member_name, receiver))
        || (view_factory_names_view_at(member_name, index) && receiver.is_view_factory())
    {
        Some(StringKeyKind::View)
    } else if first && names_command(member_name, receiver) {
        Some(StringKeyKind::Command)
    } else if ability_arguments(member_name, receiver).is_some_and(|args| args.ability == index) {
        Some(StringKeyKind::GateAbility)
    } else if index == MORPH_TYPES_ARG && is_morph_types_query_method(member_name) {
        Some(StringKeyKind::MorphAlias)
    } else {
        None
    }
}

/// Emit the Laravel string spans an instance method call carries: config
/// keys, container bindings, route and view names, Artisan commands,
/// authorization abilities, and morph aliases.
pub(in crate::symbol_map::extraction) fn emit_laravel_method_call_spans<'a>(
    object: &'a Expression<'a>,
    member_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    let receiver = AstReceiver {
        object,
        in_console_command: ctx.in_console_command,
        in_mailable: ctx.in_mailable,
        resolved_names: ctx.resolved_names,
    };
    if member_name.eq_ignore_ascii_case("macro") {
        try_emit_laravel_macro_string_span(argument_list, ctx.content, &mut ctx.spans);
    }
    if names_config_key(member_name, &receiver) {
        try_emit_laravel_config_key_span(member_name, argument_list, ctx.content, &mut ctx.spans);
    }
    if receiver.is_container() {
        try_emit_container_key_span(member_name, argument_list, ctx.content, &mut ctx.spans);
    }
    if names_route(member_name, &receiver) {
        try_emit_laravel_string_span(
            StringKeyKind::Route,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    if is_request_route_pattern_method(member_name) {
        try_emit_laravel_string_spans_all(
            StringKeyKind::Route,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // `view`, `make`, and `first` are far too common as method names to
    // recognise on their own, so a template is named syntactically only
    // through a mailable's `$this` or a bare `view()` call.
    let mut named_syntactically = true;
    if names_mailable_view(member_name, &receiver) {
        try_emit_laravel_string_span(
            StringKeyKind::View,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    } else if receiver.is_view_factory()
        && let Some(index) = view_factory_method_name_index(member_name)
    {
        try_emit_laravel_view_span_at(index, argument_list, ctx.content, &mut ctx.spans);
    } else if receiver.is_view_factory() && is_render_each_method(member_name) {
        try_emit_render_each_view_spans(argument_list, ctx.content, &mut ctx.spans);
    } else {
        named_syntactically = false;
    }
    // Nothing the receiver is spelled as says it renders, so whether this
    // names a template at all is a question about its type — recorded here
    // and answered lazily.
    if !named_syntactically
        && let Some((view_receiver, name_indices)) = view_receiver_method(member_name)
    {
        record_view_receiver_sites(
            view_receiver,
            name_indices,
            argument_list,
            ctx.content,
            &mut ctx.typed_receiver_sites,
        );
    }
    if names_command(member_name, &receiver) {
        try_emit_laravel_string_span(
            StringKeyKind::Command,
            argument_list,
            ctx.content,
            &mut ctx.spans,
        );
    }
    // A console command reading its own arguments and options.
    if receiver.is_console_command() {
        match member_name.to_ascii_lowercase().as_str() {
            "argument" | "hasargument" | "getargument" => {
                try_emit_command_own_param_span(false, argument_list, ctx.content, &mut ctx.spans);
            }
            "option" | "hasoption" | "getoption" => {
                try_emit_command_own_param_span(true, argument_list, ctx.content, &mut ctx.spans);
            }
            _ => {}
        }
    }
    emit_gate_ability_spans_for_method(&receiver, member_name, argument_list, ctx);
    // `$query->whereHasMorph('commentable', ['post'], …)` resolves each type
    // through the morph map, so a literal there is an alias.  Keyed on the
    // method name alone: every method in the family is Eloquent-specific
    // enough that a same-named method elsewhere is vanishingly unlikely.
    if is_morph_types_query_method(member_name) {
        try_emit_morph_type_spans(argument_list, MORPH_TYPES_ARG, ctx.content, &mut ctx.spans);
    }
}

/// Emit ability spans for an instance-method authorization check (see
/// [`ability_arguments`]), and for the `can:ability,Model` parameter of a
/// route registration's or a controller's `->middleware()`.
fn emit_gate_ability_spans_for_method<'a>(
    receiver: &AstReceiver<'a>,
    member_name: &str,
    argument_list: &'a ArgumentList<'a>,
    ctx: &mut ExtractionCtx<'a>,
) {
    if member_name.eq_ignore_ascii_case("middleware") {
        let route_receiver = receiver.roots_at_route();
        if !route_receiver && !receiver.is_this() {
            return;
        }
        let span_start = ctx.spans.len();
        try_emit_can_middleware_spans(argument_list, route_receiver, ctx.content, &mut ctx.spans);
        defer_on_route_root(receiver, span_start, ctx);
        return;
    }

    let Some(args) = ability_arguments(member_name, receiver) else {
        return;
    };
    let span_start = ctx.spans.len();
    try_emit_gate_ability_spans(
        argument_list,
        args.ability,
        args.model,
        gate_ability_role(member_name),
        ctx.content,
        &mut ctx.spans,
        &mut ctx.gate_subjects,
    );
    // Only a route registration's check names no model.
    if args.model.is_none() {
        defer_on_route_root(receiver, span_start, ctx);
    }
}

/// Defer the spans pushed since `span_start` until a chain rooted at the
/// optional global `Route` alias is known to reach the facade.
fn defer_on_route_root(receiver: &AstReceiver<'_>, span_start: usize, ctx: &mut ExtractionCtx<'_>) {
    if let Some(dependency) = receiver
        .resolved_names
        .and_then(|names| route_facade_root_dependency(receiver.object, names))
    {
        ctx.defer_string_key_spans_since(span_start, dependency);
    }
}
