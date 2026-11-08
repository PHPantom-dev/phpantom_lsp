//! AST extraction for the symbol map.
//!
//! This module walks the `mago_syntax` AST and emits [`SymbolSpan`],
//! [`VarDefSite`], [`TemplateParamDef`], and [`CallSite`] entries for
//! every navigable symbol occurrence.  The entry point is
//! [`extract_symbol_map`].

use mago_span::HasSpan;
use mago_syntax::cst::*;

use super::docblock::{
    class_ref_span, class_ref_span_ctx, extract_comment_see_symbols, extract_docblock_symbols,
    extract_docblock_symbols_covering, get_docblock_text_with_offset, is_navigable_type,
};
use super::morph_columns::{record_morph_column_call, record_morph_column_comparison};
use super::{
    CallArg, CallArgKind, CallExpression, CallSites, ClassRefContext, DocblockMemberRef,
    MorphColumnSite, NestedRanges, ReceiverClass, SelfStaticParentKind, SubjectText, SymbolKind,
    SymbolMap, SymbolSpan, TemplateParamDef, TypedReceiverSite, UntypedClosureSite, VarDefKind,
    VarDefSite,
};
use crate::atom::{bytes_to_str, literal_bytes_to_str};
use crate::names::OwnedResolvedNames;
use crate::util::strip_fqn_prefix;

// ─── Extraction context ─────────────────────────────────────────────────────

/// Bundles the mutable accumulators and read-only context threaded through
/// every `extract_from_*` function.
///
/// Before this struct existed, each extractor took 7–8 parameters (the five
/// `Vec`s plus `trivias`, `content`, and sometimes `scope_start`).  Grouping
/// them here eliminates the `#[allow(clippy::too_many_arguments)]` annotations
/// that were required on 19 functions and makes it trivial to add new
/// accumulated data in the future without touching every call site.
struct ExtractionCtx<'a> {
    /// Navigable symbol spans (class refs, member accesses, variables, …).
    spans: Vec<SymbolSpan>,
    /// Variable definition sites (assignments, parameters, foreach, …).
    var_defs: Vec<VarDefSite>,
    /// Scope ranges `(start, end)` for functions, methods, closures, etc.
    scopes: Vec<(u32, u32)>,
    /// Scope start offsets of arrow functions (inheriting scopes).
    arrow_fn_scopes: Vec<u32>,
    /// Body boundaries `(body_start, body_end)` for closures and arrow fns.
    /// For closures the body start is the `{` offset; for arrow functions
    /// it is the `=>` token offset.  Used by signature help suppression.
    body_scopes: Vec<(u32, u32)>,
    /// Narrowing block boundaries `(start, end)` for if-body, elseif-body,
    /// else-body, match-arm, and switch-case blocks.  Used by the
    /// diagnostic subject cache to determine whether two variable accesses
    /// are in the same narrowing context.  Accesses in the same block get
    /// the same instanceof narrowing applied and can share a cache entry.
    narrowing_blocks: Vec<(u32, u32)>,
    /// Offsets of `assert($var instanceof ...)` expression statements.
    /// Used as sequential narrowing boundaries in the diagnostic cache.
    assert_narrowing_offsets: Vec<u32>,
    /// `@template` parameter definitions with their scoping ranges.
    template_defs: Vec<TemplateParamDef>,
    /// Call-site records for signature help and conditional return types.
    call_sites: CallSites,
    /// `(start, args_start)` of each call written as a statement of its own.
    discarded_calls: Vec<(u32, u32)>,
    /// `(start, args_start)` of each function, method, and static method
    /// call.
    value_calls: Vec<(u32, u32)>,
    /// `args_start` of each call whose value nothing reads.
    unused_result_calls: Vec<u32>,
    /// Ranges where `break` is valid (loops and `switch`).
    breakable_scopes: Vec<(u32, u32)>,
    /// Ranges where `continue` is valid (loops only).
    loop_scopes: Vec<(u32, u32)>,
    /// Ranges of `switch` bodies (where `case/default` labels are valid).
    switch_scopes: Vec<(u32, u32)>,
    /// Ranges of static method bodies `(start_offset, end_offset)`.
    /// Used to detect whether `$this` is unavailable at a given offset.
    static_method_scopes: Vec<(u32, u32)>,
    /// Ranges of non-static (instance) method bodies.
    instance_method_scopes: Vec<(u32, u32)>,
    /// Trivia (comments, whitespace) from the parsed program.
    trivias: &'a [Trivia<'a>],
    /// The full source text of the file being extracted.
    content: &'a str,
    /// Semantic names resolved by `mago-names` for this file. Production
    /// indexing supplies this so Laravel-specific syntax does not guess from
    /// an alias or a namespace-local homonym. Syntax-only tests omit it.
    resolved_names: Option<&'a OwnedResolvedNames>,
    /// String keys whose meaning depends on a workspace symbol being
    /// absent. They are activated after the batch publishes its declarations.
    conditional_string_key_spans: Vec<super::ConditionalStringKeySpan>,
    /// Closures and arrow functions passed as arguments to callable-typed
    /// parameters, used by inlay hints.
    untyped_closure_sites: Vec<UntypedClosureSite>,
    /// String keys whose receiver only a type settles, left for
    /// `Backend::typed_receiver_spans_for` to confirm.
    typed_receiver_sites: Vec<TypedReceiverSite>,
    /// Possible morph aliases whose model and type column are resolved lazily.
    morph_column_sites: Vec<MorphColumnSite>,
    /// The model argument of each authorization check that named one.
    gate_subjects: Vec<crate::symbol_map::GateSubject>,
    /// The Messenger handlers the file's `#[AsMessageHandler]` attributes
    /// register.
    messenger_handlers: Vec<crate::symbol_map::MessengerHandlerSite>,
    /// The Twig filters and functions the file declares.
    twig_callables: Vec<crate::symbol_map::TwigCallableSite>,
    /// The variables the file's renders hand Twig templates.
    twig_contexts: Vec<crate::symbol_map::TwigContextSite>,
    /// What the file's PHP configurator calls declare a service or a
    /// parameter as.
    container_values: Vec<crate::symbol_map::ContainerValueSite>,
    /// The services the file's PHP configurator calls say decorate another.
    service_decorations: Vec<crate::symbol_map::ServiceDecoration>,
    /// The entity types the file's entity classes declare.
    entity_types: Vec<crate::symbol_map::EntityTypeSite>,
    /// The classes the file's Drupal plugin declarations are written on.
    drupal_plugin_classes: Vec<crate::symbol_map::PluginClassSite>,
    drupal_plugin_managers: Vec<crate::symbol_map::PluginManagerSite>,
    drupal_plugins: Vec<crate::symbol_map::PluginDeclarationSite>,
    /// Whether the file is a Drupal site's and says `#type`, the key a
    /// render array names its element under.  Spares every other file's
    /// arrays and assignments the look.
    drupal_type_keys: bool,
    /// Likewise for `#theme`, the key a render array names its theme hook
    /// under, which `#theme_wrappers` starts with.
    drupal_theme_keys: bool,
    drupal_route_patterns: Vec<String>,
    drupal_route_aliases: Vec<String>,
    /// The Drupal routes the file's `#[Route]` attributes declare.
    drupal_routes: Vec<crate::symbol_map::DrupalRouteSite>,
    drupal_hooks: Vec<crate::symbol_map::HookSite>,
    drupal_computed_hooks: Vec<crate::symbol_map::ComputedHookSite>,
    /// What the file holds of Drupal hooks, which its path says.
    drupal_file: crate::drupal::hooks::DrupalFile,
    /// A procedural function with `#[ProceduralHookScanStop]` was read:
    /// Drupal's hook collector reads no function after it.
    drupal_hook_scan_stopped: bool,
    /// Current conditional nesting depth (if/else, switch, while, for, etc.).
    /// Incremented when entering a conditional block, decremented when leaving.
    cond_nesting_depth: u16,
    /// Stack of block-end offsets for each conditional nesting level.
    /// The top of the stack is the end of the innermost conditional block.
    cond_block_end_stack: Vec<u32>,
    /// Whether the file imports from `Illuminate\Container\Attributes\`
    /// (checked once lazily, cached for all attribute inspections).
    has_laravel_container_attrs: Option<bool>,
    /// Whether the file imports from `PHPUnit\Framework\Attributes\`, cached
    /// the same way as [`Self::has_laravel_container_attrs`].
    has_phpunit_attrs: Option<bool>,
    /// Whether the file imports from `Illuminate\Foundation\Http\Attributes\`,
    /// cached the same way.  Gates recognition of a short `#[RedirectToRoute]`
    /// as a route-name reference.
    has_laravel_http_attrs: Option<bool>,
    /// Whether the members currently being extracted belong to a class that
    /// syntactically extends an Artisan console command (or is `#[AsCommand]`
    /// decorated).  Gates recognition of `$this->call('cmd')` /
    /// `$this->callSilently('cmd')` as command-name references.
    in_console_command: bool,
    /// Whether the members currently being extracted belong to a class that
    /// syntactically extends `Illuminate\Mail\Mailable`.  Gates recognition
    /// of `$this->view('emails.shipped')` and of `new Content(view: …)` as
    /// view-name references.
    in_mailable: bool,
    /// Whether the members currently being extracted belong to a class that
    /// syntactically extends a `Controller`.  Gates recognition of
    /// `$this->middleware('auth')` as a middleware name.
    in_controller: bool,
    /// The `@coversDefaultClass` of the class whose members are currently
    /// being extracted.  It is what gives `@covers ::name` on a test method
    /// a subject; without one that shape names a global function.
    covers_default_class: Option<crate::atom::Atom>,
    /// Whether the project builds a Symfony service container.  Gates the
    /// container calls recorded for the type engine to confirm, which
    /// `get()` and `set()` would otherwise record all over any other
    /// project.
    symfony: bool,
    /// Whether the project builds a Symfony service container, as Symfony
    /// and Drupal both do.  Gates the container lookups recorded for the
    /// type engine to confirm as naming a service or a parameter.
    service_container: bool,
    /// Whether the project is a Drupal site.  Gates the lookups only
    /// Drupal makes, such as `\Drupal::service()`, the entity types an
    /// entity class declares, and the entity type IDs its manager reads.
    drupal: bool,
    /// Whether the file is a Drupal site's and says `#attached`, the only
    /// key of a render array its libraries are attached under.  Spares
    /// every other file's arrays and assignments the look.
    drupal_attached: bool,
    /// Whether the project uses Twig, outside Symfony too.  Gates the
    /// `render()` calls recorded for the type engine to confirm as naming
    /// a template.
    twig: bool,
    /// The directory of the file, when the project is a Yii application.
    /// Gates the render calls recorded as naming a view, and is what a
    /// path built from `__DIR__` evaluates against.
    yii_dir: Option<&'a std::path::Path>,
    /// What a view name in the code being extracted is relative to.
    yii_view_scope: YiiViewScope,
    /// The category the translators of the class being extracted look
    /// message IDs up in when a call names none.
    yii_category: YiiCategory,
    /// Where the name of the class being extracted is written, for the
    /// controller a renderer's `withController($this)` names and the class a
    /// Twig extension's `[$this, 'method']` callable calls.
    class_name_offset: Option<u32>,
    /// Whether the file names anything from Symfony's container
    /// configurator namespace, checked once lazily.
    uses_symfony_configurators: Option<bool>,
    /// Likewise for Symfony's routing configurator namespace.
    uses_symfony_routing_configurators: Option<bool>,
    /// Whether the file imports the class of Symfony's `@Route` docblock
    /// annotation, checked once lazily.
    uses_route_annotations: Option<bool>,
    /// The parameters typed as Symfony's routing configurator, by name,
    /// whose `import()` loads routes from elsewhere.
    routing_configurator_params: Vec<String>,
    /// The variables holding a routing configurator, by name, with the
    /// name prefix the routes added on them take, or `None` when it is
    /// not written as a literal.  The latest assignment of a name wins.
    route_prefix_vars: Vec<(String, Option<String>)>,
    /// The routes the file loads from elsewhere, each with the offset of
    /// the `import()` call a later `namePrefix()` finds it by.
    route_imports: Vec<(u32, crate::symbol_map::RouteImport)>,
    /// The `try` blocks whose `catch` copes with a missing name, by byte
    /// range, with the kinds of name it copes with (see
    /// [`crate::symbol_map::LookupGuards`]).
    lookup_guards: Vec<(u32, u32, u8)>,
    /// The variables and properties the file assigns a router or a Twig
    /// environment it builds itself, by their text, with the kind of name
    /// they hold as a guard bit (see [`crate::symbol_map::LookupGuards`]).
    own_receivers: Vec<(String, u8)>,
    /// Where the name of each lookup on something that may be one of
    /// [`Self::own_receivers`] starts, with the text of the receiver and the
    /// kind of name it looks up as a guard bit.
    own_receiver_lookups: Vec<(u32, String, u8)>,
    /// Whether the file declares a Twig loader class of its own.
    declares_twig_loader: bool,
    /// Whether the project is a Yii application, which [`Self::yii_dir`]
    /// says.  Gates the route definitions, the route arguments, and the URL
    /// generator calls recorded for the type engine to confirm.
    yii: bool,
    /// Whether the file names anything from Yii's router, checked once
    /// lazily.
    names_yii_router: Option<bool>,
    /// The byte ranges of the `Route` and `Group` chains read, whose inner
    /// calls the walk reaches afterwards (some more than once) and must not
    /// read again.
    yii_read_chains: Vec<(u32, u32)>,
    /// The routes the file declares.
    routes: Vec<crate::symbol_map::RouteSite>,
    tempest_routes: Vec<crate::tempest::routes::RouteAttribute>,
    tempest_uri_calls: Vec<u32>,
    /// The classes the file's DI definitions bind and build.
    yii_definitions: Vec<crate::symbol_map::YiiDefinitionSite>,
    /// The file's DI definition arrays that build a known class.
    yii_definition_bodies: Vec<crate::symbol_map::YiiDefinitionBody>,
    /// The relations the file's Active Record calls name.
    yii_relations: Vec<crate::symbol_map::YiiRelationSite>,
    /// Whether the file is handed the Yii `$params` (see
    /// [`Frameworks::yii_params`]).
    yii_params: bool,
    /// Whether the project uses Doctrine, which gates reading DQL.
    doctrine: bool,
    /// The DQL variables and fields read so far (see [`doctrine_dql`]).
    dql: doctrine_dql::DqlPending<'a>,
    /// The DQL variables settled once the file is read.
    dql_aliases: Vec<crate::symbol_map::DqlAliasSite>,
    /// Where the DQL the file holds stops following the grammar.
    dql_syntax_errors: Vec<crate::symbol_map::DqlSyntaxSite>,
    /// What each method declares on the builders it hands on.
    dql_builder_methods: Vec<crate::symbol_map::DqlBuilderMethod>,
    /// The DQL variables declared in each body whose undeclared variables
    /// wait on other methods.
    dql_bodies: Vec<crate::symbol_map::DqlBody>,
    /// The `getQuery()` calls made on builders other methods hand on.
    dql_runs: Vec<crate::symbol_map::DqlRun>,
    /// The subject texts of the method chains read so far.
    spine_texts: SpineTexts,
}

/// A variable token's name without its `$`.
fn bare_variable_name(raw: &[u8]) -> String {
    let name = bytes_to_str(raw);
    name.strip_prefix('$').unwrap_or(name).to_string()
}

/// Whether a string literal's `name` is a plain PHP identifier, so that
/// prose such as `[$a, 'some text']` or `#[CoversMethod(Foo::class, 'not a
/// name')]` is not turned into a symbol.
fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The [`SymbolKind::ClassReference`] span for a class name written as an
/// identifier in code.
fn identifier_class_ref(ident: &Identifier<'_>, context: ClassRefContext) -> SymbolSpan {
    let span = ident.span();
    class_ref_span_ctx(
        span.start.offset,
        span.end.offset,
        bytes_to_str(ident.value()),
        context,
    )
}

impl<'a> ExtractionCtx<'a> {
    fn resolved_name_at(&self, offset: u32) -> Option<&'a str> {
        self.resolved_names.and_then(|names| names.get(offset))
    }

    /// Record that the variable `name` (without `$`) is defined by the
    /// token at `offset`, visible from `effective_from` in the scope that
    /// starts at `scope_start`, inside the conditional block the walk is
    /// currently in.
    fn push_var_def(
        &mut self,
        offset: u32,
        name: String,
        kind: VarDefKind,
        scope_start: u32,
        effective_from: u32,
    ) {
        self.var_defs.push(VarDefSite {
            offset,
            name,
            kind,
            scope_start,
            effective_from,
            nesting_depth: self.cond_nesting_depth,
            block_end: self
                .cond_block_end_stack
                .last()
                .copied()
                .unwrap_or(u32::MAX),
        });
    }

    /// Record a variable token `[start, end)` that both navigates (a
    /// `Variable` span) and defines the variable, effective at once.
    fn push_variable_definition(
        &mut self,
        (start, end): (u32, u32),
        name: String,
        kind: VarDefKind,
        scope_start: u32,
    ) {
        self.spans.push(SymbolSpan {
            start,
            end,
            kind: SymbolKind::Variable {
                name: crate::atom::atom(&name),
            },
        });
        self.push_var_def(start, name, kind, scope_start, start);
    }

    /// Move string-key spans emitted since `start` into the dormant
    /// candidate list. The tail is normally one span, so removing in place
    /// avoids allocating a temporary vector on this already-rare path.
    fn defer_string_key_spans_since(
        &mut self,
        start: usize,
        dependency: super::StringKeyDependency,
    ) {
        let spans = &mut self.spans;
        let candidates = &mut self.conditional_string_key_spans;
        for span in spans.drain(start..) {
            debug_assert!(matches!(&span.kind, SymbolKind::StringKey { .. }));
            candidates.push(super::ConditionalStringKeySpan::new(dependency, span));
        }
    }
}

mod class_like;
mod doctrine_dql;
mod doctrine_fields;
mod drupal;
mod expressions;
mod keywords;
pub(crate) mod laravel;
mod mock_methods;
mod phpunit;
mod statements;
mod subject_text;
mod symfony;
mod tempest;
mod twig;
mod twig_context;
mod wordpress;
mod yii;
mod yii_active_record;
mod yii_definitions;
mod yii_routes;
mod yii_translations;

use class_like::*;
use doctrine_dql::*;
pub(crate) use doctrine_dql::{PathEvent, undeclared_on_some_path};
use doctrine_fields::*;
pub(crate) use drupal::name_shape;
use drupal::*;
use expressions::*;
use keywords::*;
use laravel::*;
use mock_methods::*;
use phpunit::*;
use statements::*;
use subject_text::*;
use symfony::*;
use twig::*;
use twig_context::*;
use wordpress::*;
use yii::*;
use yii_active_record::*;
use yii_definitions::*;
use yii_routes::*;
use yii_translations::*;

pub(crate) use drupal::{hook_invocation, is_entity_type_attribute};
pub(crate) use symfony::{
    configurator_function_kind, is_autowire_attribute, is_bus_name_stamp,
    is_event_listener_attribute, is_message_handler_attribute, is_template_attribute,
    is_translatable_message, is_translate_function, symfony_receiver_method,
};
pub(crate) use yii::{chain_view_base, class_view_base};

/// Descend generically through a node the typed dispatchers have no arm for,
/// re-entering the typed extractors at the next expression or statement
/// boundary so new AST variants cannot create blind spots.
///
/// The explicit dispatchers thread real scope state (`scope_start`,
/// conditional nesting, the various `*_scopes` vectors); this fallback does
/// not reproduce it.  It only rescues variants that would otherwise contribute
/// nothing at all, so partial coverage (symbol spans, but no keyword emission
/// or var-def sites) is the intended result for those.
fn descend_unhandled<'a>(node: Node<'a, 'a>, ctx: &mut ExtractionCtx<'a>, scope_start: u32) {
    node.visit_children(|child| match child {
        Node::Expression(e) => extract_from_expression(e, ctx, scope_start),
        Node::Statement(s) => extract_from_statement(s, ctx, scope_start),
        other => descend_unhandled(other, ctx, scope_start),
    });
}

// ─── AST extraction ─────────────────────────────────────────────────────────

/// The frameworks whose names a symbol map records, by what the project
/// uses.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Frameworks {
    /// The project is a Symfony application.
    pub symfony: bool,
    /// The project builds a Symfony service container: a Symfony or a
    /// Drupal project.
    pub service_container: bool,
    /// The project is a Drupal project.
    pub drupal: bool,
    /// The project uses Twig.
    pub twig: bool,
    /// The project uses Doctrine.
    pub doctrine: bool,
    /// The file is a Yii configuration file `yiisoft/config` hands the
    /// `$params`, whose reads of it are recorded.
    pub yii_params: bool,
}

#[cfg(test)]
impl Frameworks {
    /// Every framework, for tests that do not care which project they are in.
    pub(crate) const ALL: Self = Self {
        symfony: true,
        service_container: true,
        drupal: true,
        twig: true,
        doctrine: true,
        yii_params: false,
    };
}

/// Build a [`SymbolMap`] from a parsed PHP program.
///
/// Walks every statement recursively and emits [`SymbolSpan`] entries for
/// every navigable symbol occurrence.
#[cfg(test)]
pub(crate) fn extract_symbol_map(program: &Program<'_>, content: &str) -> SymbolMap {
    extract_symbol_map_inner(
        program,
        content,
        None,
        Frameworks::ALL,
        None,
        crate::drupal::hooks::DrupalFile::Other,
    )
}

/// Build a [`SymbolMap`] using the semantic names produced by `mago-names`.
#[cfg(test)]
pub(crate) fn extract_symbol_map_with_resolved_names(
    program: &Program<'_>,
    content: &str,
    resolved_names: &OwnedResolvedNames,
) -> SymbolMap {
    let mut map = extract_symbol_map_inner(
        program,
        content,
        Some(resolved_names),
        Frameworks::ALL,
        None,
        crate::drupal::hooks::DrupalFile::Other,
    );
    map.refresh_conditional_string_key_spans(None, |_| false);
    map
}

/// Build a semantic map while retaining workspace-dependent Laravel spans as
/// dormant candidates for the publication lifecycle to settle.
///
/// `frameworks` says which frameworks' names to record, and `yii_dir` is the
/// file's directory when the project is a Yii application.
#[cfg(test)]
pub(crate) fn extract_symbol_map_for_index(
    program: &Program<'_>,
    content: &str,
    resolved_names: &OwnedResolvedNames,
    frameworks: Frameworks,
    yii_dir: Option<&std::path::Path>,
) -> SymbolMap {
    extract_symbol_map_inner(
        program,
        content,
        Some(resolved_names),
        frameworks,
        yii_dir,
        crate::drupal::hooks::DrupalFile::Other,
    )
}

/// [`extract_symbol_map_for_index`] for a file whose path says what it
/// holds of Drupal hooks.
pub(crate) fn extract_symbol_map_for_file(
    program: &Program<'_>,
    content: &str,
    resolved_names: &OwnedResolvedNames,
    frameworks: Frameworks,
    yii_dir: Option<&std::path::Path>,
    drupal_file: crate::drupal::hooks::DrupalFile,
) -> SymbolMap {
    extract_symbol_map_inner(
        program,
        content,
        Some(resolved_names),
        frameworks,
        yii_dir,
        drupal_file,
    )
}

fn extract_symbol_map_inner(
    program: &Program<'_>,
    content: &str,
    resolved_names: Option<&OwnedResolvedNames>,
    frameworks: Frameworks,
    yii_dir: Option<&std::path::Path>,
    drupal_file: crate::drupal::hooks::DrupalFile,
) -> SymbolMap {
    let Frameworks {
        symfony,
        service_container,
        drupal,
        twig,
        doctrine,
        yii_params,
    } = frameworks;
    let mut ctx = ExtractionCtx {
        spans: Vec::new(),
        var_defs: Vec::new(),
        scopes: Vec::new(),
        arrow_fn_scopes: Vec::new(),
        body_scopes: Vec::new(),
        narrowing_blocks: Vec::new(),
        assert_narrowing_offsets: Vec::new(),
        template_defs: Vec::new(),
        call_sites: CallSites::default(),
        discarded_calls: Vec::new(),
        value_calls: Vec::new(),
        unused_result_calls: Vec::new(),
        breakable_scopes: Vec::new(),
        loop_scopes: Vec::new(),
        switch_scopes: Vec::new(),
        static_method_scopes: Vec::new(),
        instance_method_scopes: Vec::new(),
        trivias: program.trivia.as_slice(),
        content,
        resolved_names,
        conditional_string_key_spans: Vec::new(),
        untyped_closure_sites: Vec::new(),
        typed_receiver_sites: Vec::new(),
        morph_column_sites: Vec::new(),
        gate_subjects: Vec::new(),
        messenger_handlers: Vec::new(),
        twig_callables: Vec::new(),
        twig_contexts: Vec::new(),
        container_values: Vec::new(),
        service_decorations: Vec::new(),
        entity_types: Vec::new(),
        drupal_plugin_classes: Vec::new(),
        drupal_plugin_managers: Vec::new(),
        drupal_plugins: Vec::new(),
        drupal_type_keys: drupal && memchr::memmem::find(content.as_bytes(), b"#type").is_some(),
        drupal_theme_keys: drupal && memchr::memmem::find(content.as_bytes(), b"#theme").is_some(),
        drupal_route_patterns: Vec::new(),
        drupal_route_aliases: Vec::new(),
        drupal_routes: Vec::new(),
        drupal_hooks: Vec::new(),
        drupal_computed_hooks: Vec::new(),
        drupal_file,
        drupal_hook_scan_stopped: false,
        cond_nesting_depth: 0,
        cond_block_end_stack: Vec::new(),
        has_laravel_container_attrs: None,
        has_phpunit_attrs: None,
        has_laravel_http_attrs: None,
        in_console_command: false,
        in_mailable: false,
        in_controller: false,
        covers_default_class: None,
        symfony,
        service_container,
        drupal,
        drupal_attached: drupal && memchr::memmem::find(content.as_bytes(), b"#attached").is_some(),
        twig,
        uses_symfony_configurators: None,
        uses_symfony_routing_configurators: None,
        uses_route_annotations: None,
        routing_configurator_params: Vec::new(),
        route_prefix_vars: Vec::new(),
        route_imports: Vec::new(),
        lookup_guards: Vec::new(),
        own_receivers: Vec::new(),
        own_receiver_lookups: Vec::new(),
        declares_twig_loader: false,
        yii_dir,
        yii_view_scope: YiiViewScope::Template,
        yii_category: YiiCategory::Default,
        class_name_offset: None,
        yii: yii_dir.is_some(),
        names_yii_router: None,
        yii_read_chains: Vec::new(),
        routes: Vec::new(),
        tempest_routes: Vec::new(),
        tempest_uri_calls: Vec::new(),
        yii_definitions: Vec::new(),
        yii_definition_bodies: Vec::new(),
        yii_relations: Vec::new(),
        yii_params,
        doctrine,
        dql: Default::default(),
        dql_aliases: Vec::new(),
        dql_syntax_errors: Vec::new(),
        dql_builder_methods: Vec::new(),
        dql_bodies: Vec::new(),
        dql_runs: Vec::new(),
        spine_texts: SpineTexts::default(),
    };

    note_dql_body(0, program.statements.as_slice(), &mut ctx);
    for stmt in program.statements.iter() {
        extract_from_statement(stmt, &mut ctx, 0);
    }
    apply_lookup_guards(&mut ctx);
    finish_dql(&mut ctx);

    // ── Sweep all docblock trivia for floating references ───────────
    // Docblocks attached to classes, functions, methods, properties, and
    // certain statements are already processed during the AST walk above.
    // However, docblocks in other positions (e.g. inline `/** @see ... */`
    // inside array literals or after expressions) are never visited.
    // Scan every docblock trivia entry and extract symbols; the dedup
    // step below removes any duplicates from already-processed docblocks.
    // Plain comments are never attached to a node, so their `@see` tags
    // are only found here.
    for t in program.trivia.iter() {
        match t.kind {
            TriviaKind::DocBlockComment => {
                let _found = extract_docblock_symbols(
                    bytes_to_str(t.value),
                    t.span.start.offset,
                    &mut ctx.spans,
                );
            }
            TriviaKind::SingleLineComment
            | TriviaKind::HashComment
            | TriviaKind::MultiLineComment => {
                extract_comment_see_symbols(
                    bytes_to_str(t.value),
                    t.span.start.offset,
                    &mut ctx.spans,
                );
            }
            TriviaKind::WhiteSpace => {}
        }
    }

    // Emit comment spans for all comment trivia so semantic tokens
    // can highlight comments in Blade files.  For docblock comments,
    // also emit keyword spans for PHPDoc tags.
    //
    // Multi-line block comments are split into one span per line so that
    // the semantic token layer can emit correct per-line lengths without
    // any post-processing.  The LSP protocol requires token `length` to
    // describe characters on a single line only.
    for t in program.trivia.iter() {
        if t.kind.is_comment() {
            let mut byte_cursor = t.span.start.offset as usize;
            for line_text in bytes_to_str(t.value).split('\n') {
                // `line_text` may end with '\r' on Windows; strip it for
                // length calculation but keep the byte advance correct.
                let display = line_text.trim_end_matches('\r');
                let display_len = display.len() as u32;
                if display_len > 0 {
                    ctx.spans.push(SymbolSpan {
                        start: byte_cursor as u32,
                        end: byte_cursor as u32 + display_len,
                        kind: SymbolKind::Comment,
                    });
                }
                // Advance past this segment plus the '\n' (line_text
                // includes '\r' if present, so +1 covers just the LF).
                byte_cursor += line_text.len() + 1;
            }
            if t.kind == TriviaKind::DocBlockComment {
                emit_phpdoc_tag_keywords(
                    bytes_to_str(t.value),
                    t.span.start.offset,
                    &mut ctx.spans,
                );
            }
        }
    }

    // Sort by start offset for binary search.
    ctx.spans.sort_by_key(|s| s.start);

    // Deduplicate overlapping spans (keep the first / most specific).  One
    // piece of text can declare several names of a kind, as a route
    // declaration declares the names Symfony derives from it, and each
    // stays a declaration.
    ctx.spans.dedup_by(|b, a| {
        a.start == b.start && a.end == b.end && !crate::symbol_map::declares_another_name(a, b)
    });
    // Every workspace file keeps its map for the whole session, so drop
    // the growth slack and the room the dedup freed, and hold the
    // synthesised subjects in one allocation.
    ctx.spans.shrink_to_fit();
    SubjectText::pool(&mut ctx.spans);
    ctx.var_defs.shrink_to_fit();

    // Sort var_defs by (scope_start, offset) for efficient lookup.
    ctx.var_defs.sort_by(|a, b| {
        a.scope_start
            .cmp(&b.scope_start)
            .then(a.offset.cmp(&b.offset))
    });

    let mut var_defs_by_name: Vec<u32> = (0..ctx.var_defs.len() as u32).collect();
    var_defs_by_name.sort_by(|&a, &b| {
        let (a, b) = (&ctx.var_defs[a as usize], &ctx.var_defs[b as usize]);
        (a.scope_start, &a.name, a.offset).cmp(&(b.scope_start, &b.name, b.offset))
    });

    ctx.assert_narrowing_offsets.sort();
    ctx.arrow_fn_scopes.sort_unstable();

    // Sort template_defs by name_offset for binary search / reverse scan.
    ctx.template_defs.sort_by_key(|d| d.name_offset);

    // Sort call_sites by args_start for reverse-scan lookup.
    ctx.call_sites.finish();
    ctx.discarded_calls
        .sort_unstable_by_key(|&(_, args_start)| args_start);
    ctx.discarded_calls.shrink_to_fit();
    let used_result_calls = used_result_calls(ctx.value_calls, ctx.unused_result_calls);
    ctx.breakable_scopes.sort_by_key(|s| s.0);
    ctx.loop_scopes.sort_by_key(|s| s.0);
    ctx.switch_scopes.sort_by_key(|s| s.0);
    ctx.static_method_scopes.sort_by_key(|s| s.0);
    ctx.typed_receiver_sites.sort_by_key(|s| s.start);
    ctx.morph_column_sites.sort_by_key(|s| s.start);
    ctx.container_values.sort_by_key(|s| s.start);
    ctx.entity_types.sort_by_key(|s| s.start);
    ctx.drupal_plugin_classes.sort_by_key(|s| s.start);
    ctx.drupal_plugins.sort_by_key(|s| s.start);
    ctx.drupal_hooks.sort_by_key(|s| s.start);
    ctx.tempest_uri_calls.sort_unstable();
    ctx.drupal_hooks.dedup_by_key(|s| s.start);
    ctx.routes.shrink_to_fit();

    let mut member_access_indices: crate::atom::AtomMap<Vec<usize>> =
        crate::atom::AtomMap::default();
    for (idx, span) in ctx.spans.iter().enumerate() {
        if let SymbolKind::MemberAccess { member_name, .. } = &span.kind {
            member_access_indices
                .entry(*member_name)
                .or_default()
                .push(idx);
        }
    }

    SymbolMap {
        spans: ctx.spans,
        conditional_string_key_spans: ctx.conditional_string_key_spans,
        member_access_indices,
        var_defs: ctx.var_defs,
        var_defs_by_name,
        scopes: NestedRanges::new(ctx.scopes),
        arrow_fn_scopes: ctx.arrow_fn_scopes,
        body_scopes: ctx.body_scopes,
        narrowing_blocks: NestedRanges::new(ctx.narrowing_blocks),
        assert_narrowing_offsets: ctx.assert_narrowing_offsets,
        template_defs: ctx.template_defs,
        call_sites: ctx.call_sites,
        discarded_calls: ctx.discarded_calls,
        used_result_calls,
        breakable_scopes: ctx.breakable_scopes,
        loop_scopes: ctx.loop_scopes,
        switch_scopes: ctx.switch_scopes,
        static_method_scopes: ctx.static_method_scopes,
        instance_method_scopes: ctx.instance_method_scopes,
        untyped_closure_sites: ctx.untyped_closure_sites,
        typed_receiver_sites: ctx.typed_receiver_sites,
        morph_column_sites: ctx.morph_column_sites,
        gate_subjects: ctx.gate_subjects,
        messenger_handlers: ctx.messenger_handlers,
        twig_callables: ctx.twig_callables,
        twig_contexts: ctx.twig_contexts,
        container_values: ctx.container_values,
        service_decorations: ctx.service_decorations,
        entity_types: ctx.entity_types,
        drupal_plugin_classes: ctx.drupal_plugin_classes,
        drupal_plugin_managers: ctx.drupal_plugin_managers,
        drupal_plugins: ctx.drupal_plugins,
        drupal_route_patterns: ctx.drupal_route_patterns,
        drupal_route_aliases: ctx.drupal_route_aliases,
        drupal_routes: ctx.drupal_routes,
        drupal_hooks: ctx.drupal_hooks,
        drupal_computed_hooks: ctx.drupal_computed_hooks,
        routes: ctx.routes,
        tempest_routes: ctx.tempest_routes.into_boxed_slice(),
        tempest_uri_calls: ctx.tempest_uri_calls.into_boxed_slice(),
        yii_definitions: ctx.yii_definitions,
        yii_definition_bodies: ctx.yii_definition_bodies,
        yii_relations: ctx.yii_relations,
        dql_aliases: ctx.dql_aliases,
        dql_syntax_errors: ctx.dql_syntax_errors,
        dql_builder_methods: ctx.dql_builder_methods,
        dql_bodies: ctx.dql_bodies,
        dql_runs: ctx.dql_runs,
        declares_twig_loader: ctx.declares_twig_loader,
        route_imports: ctx
            .route_imports
            .into_iter()
            .map(|(_, import)| import)
            .collect(),
        source_len: u32::try_from(content.len()).unwrap_or(u32::MAX),
        source_hash: None,
    }
}

/// The calls of `value_calls` that are not in `unused`: those whose value
/// something reads, sorted.
fn used_result_calls(mut value_calls: Vec<(u32, u32)>, mut unused: Vec<u32>) -> Vec<(u32, u32)> {
    value_calls.sort_unstable_by_key(|&(_, args_start)| args_start);
    unused.sort_unstable();
    let mut unused = unused.into_iter().peekable();
    value_calls.retain(|&(_, call)| {
        while unused.next_if(|&u| u < call).is_some() {}
        unused.peek() != Some(&call)
    });
    value_calls.shrink_to_fit();
    value_calls
}
