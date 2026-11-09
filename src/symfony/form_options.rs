//! The options a Symfony form type accepts, and the options arrays that
//! pass them.
//!
//! A form type's options are the ones its `configureOptions()` declares,
//! the ones of the type its `getParent()` names (every type but `FormType`
//! and `ButtonType` has one, `AbstractType`'s default being `FormType`),
//! and the ones of the type extensions that extend it or one of those
//! parents.  Each is read from the class's source: `setDefaults()`,
//! `setDefault()`, `setRequired()`, `setDefined()`, and `define()` on the
//! resolver the method takes, following `parent::configureOptions()` up the
//! class's own parents.
//!
//! A class whose options cannot all be read that way (a resolver handed to
//! a helper, a `setDefaults($defaults)`, a type named by a variable) makes
//! the set incomplete: what was read still completes, but nothing is
//! reported as unknown.
//!
//! The options arrays are the ones a form builder's `add()` and `create()`,
//! a form factory's `create()` family, and a controller's `createForm()` and
//! `createFormBuilder()` pass, each for the type the call names, once the
//! type engine confirms the receiver.  Completion and diagnostics read them
//! the same way (see [`options_sites`]).
//!
//! The installed packages' types and extensions are read from disk once and
//! kept until the dependencies change; the project's own are read afresh.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use parking_lot::RwLock;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::ResolvedClassLoader;

/// The type every form type descends from.
pub(crate) const FORM_TYPE: &str = "Symfony\\Component\\Form\\Extension\\Core\\Type\\FormType";
const FORM_BUILDER: &str = "Symfony\\Component\\Form\\FormBuilderInterface";
const FORM: &str = "Symfony\\Component\\Form\\FormInterface";
const FORM_FACTORY: &str = "Symfony\\Component\\Form\\FormFactoryInterface";
const CONTROLLER: &str = "Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController";
const TYPE_EXTENSION: &str = "Symfony\\Component\\Form\\FormTypeExtensionInterface";

// ─── Options arrays ─────────────────────────────────────────────────────────

/// A method that passes a form type options: on which receivers, and in
/// which parameters (by position and by name) the type and the options go.
struct OptionsCall {
    method: &'static str,
    receivers: &'static [&'static str],
    /// `None` for a call that always builds a plain form.
    type_param: Option<(usize, &'static str)>,
    /// The type when the call is not given one.  A builder guesses it from
    /// the data class, which is not followed here.
    default_type: Option<&'static str>,
    options_param: (usize, &'static str),
}

const OPTIONS_CALLS: &[OptionsCall] = &[
    OptionsCall {
        method: "add",
        receivers: &[FORM_BUILDER, FORM],
        type_param: Some((1, "type")),
        default_type: None,
        options_param: (2, "options"),
    },
    OptionsCall {
        method: "create",
        receivers: &[FORM_BUILDER],
        type_param: Some((1, "type")),
        default_type: None,
        options_param: (2, "options"),
    },
    OptionsCall {
        method: "create",
        receivers: &[FORM_FACTORY],
        type_param: Some((0, "type")),
        default_type: Some(FORM_TYPE),
        options_param: (2, "options"),
    },
    OptionsCall {
        method: "createNamed",
        receivers: &[FORM_FACTORY],
        type_param: Some((1, "type")),
        default_type: Some(FORM_TYPE),
        options_param: (3, "options"),
    },
    OptionsCall {
        method: "createBuilder",
        receivers: &[FORM_FACTORY],
        type_param: Some((0, "type")),
        default_type: Some(FORM_TYPE),
        options_param: (2, "options"),
    },
    OptionsCall {
        method: "createNamedBuilder",
        receivers: &[FORM_FACTORY],
        type_param: Some((1, "type")),
        default_type: Some(FORM_TYPE),
        options_param: (3, "options"),
    },
    OptionsCall {
        method: "createForm",
        receivers: &[CONTROLLER],
        type_param: Some((0, "type")),
        default_type: None,
        options_param: (2, "options"),
    },
    OptionsCall {
        method: "createFormBuilder",
        receivers: &[CONTROLLER],
        type_param: None,
        default_type: Some(FORM_TYPE),
        options_param: (1, "options"),
    },
];

/// The form type an options array is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormTypeName<'a> {
    /// A `Foo::class`, as written and where.
    Written(&'a str, u32),
    /// A type the call defaults to.
    Fqn(&'static str),
}

/// A string in an options array: the content range of an option's key, or
/// of a value with no key, which is an option name still being typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionString {
    pub start: u32,
    pub end: u32,
    pub is_key: bool,
}

/// Whether `content` can hold an options array a call passes a form type:
/// every such call names its type with `::class`, but for
/// `createFormBuilder()`, which always builds a plain form.
pub(crate) fn may_pass_form_options(content: &str) -> bool {
    let bytes = content.as_bytes();
    memchr::memmem::find(bytes, b"::class").is_some()
        || memchr::memmem::find(bytes, b"createFormBuilder").is_some()
}

/// An options array a call passes a form type, waiting for the type engine
/// to confirm the call's receiver is one of `receivers`.
pub(crate) struct OptionsSite<'e, 'a> {
    pub receiver: &'e Expression<'a>,
    pub receivers: &'static [&'static str],
    pub form_type: FormTypeName<'a>,
    pub strings: Vec<OptionString>,
}

/// Every options array the method calls in `program` pass a form type.  A
/// call some receivers take one way and others another (a builder's and a
/// factory's `create()`) gives a site for each.
pub(crate) fn options_sites<'e, 'a>(program: &'e Program<'a>) -> Vec<OptionsSite<'e, 'a>> {
    let mut sites = Vec::new();
    for statement in program.statements.iter() {
        mago_syntax::walker::Walker::walk_statement(&SiteFinder, statement, &mut sites);
    }
    sites
}

struct SiteFinder;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, Vec<OptionsSite<'ast, 'arena>>>
    for SiteFinder
{
    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        sites: &mut Vec<OptionsSite<'ast, 'arena>>,
    ) {
        let ClassLikeMemberSelector::Identifier(name) = &node.method else {
            return;
        };
        let name = name.value;
        for call in OPTIONS_CALLS {
            if !name.eq_ignore_ascii_case(call.method.as_bytes()) {
                continue;
            }
            let arguments = &node.argument_list;
            let Some(options) = argument(arguments, call.options_param) else {
                continue;
            };
            let Some(strings) = option_strings(options) else {
                continue;
            };
            let form_type = match call.type_param.and_then(|param| argument(arguments, param)) {
                Some(written) => match crate::parser::class_fetch_identifier(written) {
                    Some(identifier) => FormTypeName::Written(
                        bytes_to_str(identifier.value()),
                        identifier.span().start.offset,
                    ),
                    None => continue,
                },
                None => match call.default_type {
                    Some(fqn) => FormTypeName::Fqn(fqn),
                    None => continue,
                },
            };
            sites.push(OptionsSite {
                receiver: node.object,
                receivers: call.receivers,
                form_type,
                strings,
            });
        }
    }
}

/// The argument a call passes in the parameter at `index` named `name`.
fn argument<'e, 'a>(
    arguments: &'e ArgumentList<'a>,
    (index, name): (usize, &str),
) -> Option<&'e Expression<'a>> {
    for (position, argument) in arguments.arguments.iter().enumerate() {
        match argument {
            Argument::Positional(argument) if argument.ellipsis.is_some() => return None,
            Argument::Positional(argument) if position == index => return Some(argument.value),
            Argument::Named(argument) if argument.name.value == name.as_bytes() => {
                return Some(argument.value);
            }
            _ => {}
        }
    }
    None
}

/// The strings an options array holds where an option is named.
fn option_strings(array: &Expression<'_>) -> Option<Vec<OptionString>> {
    let elements = match array {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return None,
    };
    let content_range = |expr: &Expression<'_>| match expr {
        Expression::Literal(Literal::String(literal)) => Some((
            literal.span.start.offset + 1,
            literal.span.end.offset.saturating_sub(1),
        )),
        _ => None,
    };
    Some(
        elements
            .iter()
            .filter_map(|element| match element {
                ArrayElement::KeyValue(entry) => {
                    content_range(entry.key).map(|(start, end)| OptionString {
                        start,
                        end,
                        is_key: true,
                    })
                }
                ArrayElement::Value(value) => {
                    content_range(value.value).map(|(start, end)| OptionString {
                        start,
                        end,
                        is_key: false,
                    })
                }
                _ => None,
            })
            .collect(),
    )
}

// ─── Reading a form class ───────────────────────────────────────────────────

/// What a class's own `getParent()` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ParentType {
    Type(String),
    /// `null`: the class is a root type, `FormType` or `ButtonType`.
    Root,
    Unreadable,
}

/// The options a class's own `configureOptions()` declares, each with the
/// byte range of its name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ConfiguredOptions {
    pub options: Vec<(String, u32, u32)>,
    /// Whether it calls `parent::configureOptions()`.
    pub calls_parent: bool,
    /// Whether every option it declares was read.
    pub complete: bool,
}

/// The types a type extension's own `getExtendedTypes()` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtendedTypes {
    Types(Vec<String>),
    Unreadable,
}

/// What one class declares about the options of the form types it is or
/// extends, from its own methods: `None` for a method it does not declare
/// a body for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FormClassReading {
    pub get_parent: Option<ParentType>,
    pub configure: Option<ConfiguredOptions>,
    pub extended_types: Option<ExtendedTypes>,
}

/// Read the form methods `class` declares.  `class_of` gives the class a
/// `Foo::class` names.
pub(crate) fn read_form_class(
    class: &Class<'_>,
    class_of: &dyn Fn(&Expression<'_>) -> Option<String>,
) -> FormClassReading {
    let mut reading = FormClassReading::default();
    for member in class.members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let MethodBody::Concrete(body) = &method.body else {
            continue;
        };
        let name = method.name.value;
        if name.eq_ignore_ascii_case(b"getParent") {
            reading.get_parent = Some(match single_return(&body.statements) {
                Some(Expression::Literal(Literal::Null(_))) => ParentType::Root,
                Some(value) => class_of(value).map_or(ParentType::Unreadable, ParentType::Type),
                None => ParentType::Unreadable,
            });
        } else if name.eq_ignore_ascii_case(b"configureOptions") {
            reading.configure = Some(match method.parameter_list.parameters.first() {
                Some(resolver) => configured_options(&body.statements, resolver.variable.name),
                None => ConfiguredOptions {
                    complete: true,
                    ..ConfiguredOptions::default()
                },
            });
        } else if name.eq_ignore_ascii_case(b"getExtendedTypes")
            || name.eq_ignore_ascii_case(b"getExtendedType")
        {
            reading.extended_types = Some(extended_types(&body.statements, class_of));
        }
    }
    reading
}

/// The value of a body that is a single `return`.
fn single_return<'e, 'a>(
    statements: &'e Sequence<'a, Statement<'a>>,
) -> Option<&'e Expression<'a>> {
    match statements.as_slice() {
        [Statement::Return(statement)] => statement.value,
        _ => None,
    }
}

/// The types a `getExtendedTypes()` body returns or yields.
fn extended_types(
    statements: &Sequence<'_, Statement<'_>>,
    class_of: &dyn Fn(&Expression<'_>) -> Option<String>,
) -> ExtendedTypes {
    let mut types = Vec::new();
    for statement in statements.iter() {
        let read = match statement {
            Statement::Return(Return {
                value: Some(Expression::Array(array)),
                ..
            }) => array.elements.iter().all(|element| match element {
                ArrayElement::Value(value) => {
                    class_of(value.value).map(|t| types.push(t)).is_some()
                }
                _ => false,
            }),
            Statement::Return(Return {
                value: Some(value), ..
            }) => class_of(value).map(|t| types.push(t)).is_some(),
            Statement::Expression(ExpressionStatement {
                expression:
                    Expression::Yield(Yield::Value(YieldValue {
                        value: Some(value), ..
                    })),
                ..
            }) => class_of(value).map(|t| types.push(t)).is_some(),
            _ => false,
        };
        if !read {
            return ExtendedTypes::Unreadable;
        }
    }
    ExtendedTypes::Types(types)
}

/// The options a `configureOptions()` body declares on the resolver held in
/// the variable `resolver`.
fn configured_options(
    statements: &Sequence<'_, Statement<'_>>,
    resolver: &[u8],
) -> ConfiguredOptions {
    let mut configured = ConfiguredOptions {
        complete: true,
        ..ConfiguredOptions::default()
    };
    for statement in statements.iter() {
        read_configure_statement(statement, resolver, &mut configured);
    }
    // A resolver handed to anything but the parent's own method can be
    // given options there.
    let mut escapes = ResolverEscapes {
        resolver,
        passed: 0,
        to_parent: 0,
    };
    for statement in statements.iter() {
        mago_syntax::walker::MutWalker::walk_statement(&mut escapes, statement, &mut ());
    }
    if escapes.passed > escapes.to_parent {
        configured.complete = false;
    }
    configured
}

fn read_configure_statement(
    statement: &Statement<'_>,
    resolver: &[u8],
    configured: &mut ConfiguredOptions,
) {
    match statement {
        Statement::Expression(statement) => {
            read_resolver_chain(statement.expression, resolver, configured);
        }
        Statement::If(statement) => crate::parser::for_each_if_branch(statement, |statements| {
            for statement in statements {
                read_configure_statement(statement, resolver, configured);
            }
        }),
        Statement::Block(block) => {
            for statement in block.statements.iter() {
                read_configure_statement(statement, resolver, configured);
            }
        }
        Statement::Try(statement) => {
            for statement in statement.block.statements.iter() {
                read_configure_statement(statement, resolver, configured);
            }
        }
        _ => {}
    }
}

/// Read the options a call chain on the resolver declares.  The resolver's
/// setters return the resolver, so the chain goes on; `define()` returns
/// the option's own configurator.
fn read_resolver_chain(expr: &Expression<'_>, resolver: &[u8], configured: &mut ConfiguredOptions) {
    if let Expression::Call(Call::StaticMethod(call)) = expr
        && matches!(call.class, Expression::Parent(_))
        && matches!(&call.method, ClassLikeMemberSelector::Identifier(name)
            if name.value.eq_ignore_ascii_case(b"configureOptions"))
    {
        configured.calls_parent = true;
        return;
    }
    let mut calls = Vec::new();
    let Some(root) = super::forms::chain_root(expr, &mut calls) else {
        return;
    };
    if !matches!(root, Expression::Variable(Variable::Direct(variable)) if variable.name == resolver)
    {
        return;
    }
    for (name, arguments) in calls {
        let is = |method: &str| name.eq_ignore_ascii_case(method.as_bytes());
        let first = arguments.arguments.first().map(Argument::value);
        if is("setDefaults") {
            if !first.is_some_and(|array| declare_array_keys(array, configured)) {
                configured.complete = false;
            }
        } else if is("setDefault") || is("define") || is("setOptions") {
            if !first.is_some_and(|name| declare_name(name, configured)) {
                configured.complete = false;
            }
            if is("define") {
                return;
            }
        } else if is("setRequired") || is("setDefined") {
            let declared = first.is_some_and(|names| match names {
                Expression::Array(array) => array.elements.iter().all(|element| match element {
                    ArrayElement::Value(value) => declare_name(value.value, configured),
                    _ => false,
                }),
                name => declare_name(name, configured),
            });
            if !declared {
                configured.complete = false;
            }
        } else if !is_resolver_setter(name) {
            return;
        }
    }
}

/// Whether the resolver method `name` returns the resolver, so a chain can
/// go on after it.
fn is_resolver_setter(name: &[u8]) -> bool {
    [
        "setAllowedTypes",
        "addAllowedTypes",
        "setAllowedValues",
        "addAllowedValues",
        "setNormalizer",
        "addNormalizer",
        "setDeprecated",
        "setInfo",
        "setPrototype",
        "setIgnoreUndefined",
        "remove",
        "clear",
    ]
    .iter()
    .any(|setter| name.eq_ignore_ascii_case(setter.as_bytes()))
}

/// Declare the option a string names.
fn declare_name(expr: &Expression<'_>, configured: &mut ConfiguredOptions) -> bool {
    let Expression::Literal(Literal::String(literal)) = expr else {
        return false;
    };
    let Some(name) = literal.value.and_then(crate::atom::literal_bytes_to_str) else {
        return false;
    };
    configured.options.push((
        name.to_string(),
        literal.span.start.offset + 1,
        literal.span.end.offset.saturating_sub(1),
    ));
    true
}

/// Declare the options an array's keys name.
fn declare_array_keys(array: &Expression<'_>, configured: &mut ConfiguredOptions) -> bool {
    let elements = match array {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return false,
    };
    elements.iter().all(|element| match element {
        ArrayElement::KeyValue(entry) => declare_name(entry.key, configured),
        _ => false,
    })
}

/// Counts the arguments that pass the resolver on, and the ones of them
/// that go to `parent::configureOptions()`.
struct ResolverEscapes<'r> {
    resolver: &'r [u8],
    passed: usize,
    to_parent: usize,
}

impl ResolverEscapes<'_> {
    fn passes_resolver(&self, arguments: &ArgumentList<'_>) -> bool {
        arguments.arguments.iter().any(|argument| {
            matches!(argument.value(),
                Expression::Variable(Variable::Direct(variable)) if variable.name == self.resolver)
        })
    }
}

impl<'ast, 'arena> mago_syntax::walker::MutWalker<'ast, 'arena, ()> for ResolverEscapes<'_> {
    fn walk_in_argument_list(&mut self, node: &'ast ArgumentList<'arena>, _: &mut ()) {
        if self.passes_resolver(node) {
            self.passed += 1;
        }
    }

    fn walk_in_static_method_call(&mut self, node: &'ast StaticMethodCall<'arena>, _: &mut ()) {
        if matches!(node.class, Expression::Parent(_))
            && matches!(&node.method, ClassLikeMemberSelector::Identifier(name)
                if name.value.eq_ignore_ascii_case(b"configureOptions"))
            && self.passes_resolver(&node.argument_list)
        {
            self.to_parent += 1;
        }
    }
}

/// The class named `fqn` among `statements` and the namespaces they open.
fn find_class<'p, 'a>(
    statements: impl Iterator<Item = &'p Statement<'a>>,
    namespace: &str,
    fqn: &str,
) -> Option<&'p Class<'a>>
where
    'a: 'p,
{
    for statement in statements {
        match statement {
            Statement::Namespace(inner) => {
                let name = inner
                    .name
                    .as_ref()
                    .map_or("", |name| bytes_to_str(name.value()));
                if let Some(class) = find_class(inner.statements().iter(), name, fqn) {
                    return Some(class);
                }
            }
            Statement::Class(class) => {
                let short = bytes_to_str(class.name.value);
                let matches = match fqn.rsplit_once('\\') {
                    Some((space, name)) => {
                        space.eq_ignore_ascii_case(namespace.trim_matches('\\'))
                            && name.eq_ignore_ascii_case(short)
                    }
                    None => namespace.is_empty() && fqn.eq_ignore_ascii_case(short),
                };
                if matches {
                    return Some(class);
                }
            }
            _ => {}
        }
    }
    None
}

/// Read the form methods of the class `fqn` declared in `content`.
fn read_form_class_in(uri: &str, content: &str, fqn: &str) -> Option<FormClassReading> {
    let arena = mago_allocator::LocalArena::new();
    let file_id = mago_database::file::FileId::new(uri.as_bytes());
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, content.as_bytes());
    let resolver = mago_names::resolver::NameResolver::new(&arena);
    let names = crate::names::OwnedResolvedNames::from_resolved(&resolver.resolve(program));
    let class = find_class(program.statements.iter(), "", fqn)?;
    let class_of = |expr: &Expression<'_>| match expr {
        Expression::Literal(Literal::String(literal)) => literal
            .value
            .and_then(crate::atom::literal_bytes_to_str)
            .map(|name| name.trim_start_matches('\\').to_string())
            .filter(|name| !name.is_empty()),
        _ => names.class_fetch_fqn(expr),
    };
    Some(read_form_class(class, &class_of))
}

// ─── A type's options ───────────────────────────────────────────────────────

/// One class, as far as its form methods go.
#[derive(Debug)]
struct FormClass {
    uri: String,
    parent_class: Option<String>,
    uses_traits: bool,
    reading: FormClassReading,
}

/// A type extension: the class and its parents, nearest first, and the
/// types it extends.
struct TypeExtension {
    chain: Vec<Arc<FormClass>>,
    types: Vec<String>,
}

/// An option a form type accepts, and where it is declared.
#[derive(Debug, Clone)]
pub(crate) struct FormOption {
    pub name: String,
    pub uri: String,
    pub start: u32,
    pub end: u32,
}

/// Every option a form type accepts.
#[derive(Debug, Default)]
pub(crate) struct FormOptions {
    pub options: Vec<FormOption>,
    /// Whether every class that declares them was read, so that a name not
    /// among them is no option of the type's.
    pub complete: bool,
}

impl FormOptions {
    pub(crate) fn accepts(&self, name: &str) -> bool {
        self.options.iter().any(|option| option.name == name)
    }

    /// The names of the options, sorted.
    pub(crate) fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .options
            .iter()
            .map(|option| option.name.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }
}

/// The form classes and type extensions the installed packages declare,
/// once read.
#[derive(Default)]
pub(crate) struct VendorFormClasses {
    classes: RwLock<HashMap<String, Arc<FormClass>>>,
    extensions: RwLock<Option<Arc<Vec<String>>>>,
}

impl Backend {
    /// The form type `site` passes its options to, once the type engine
    /// confirms the call's receiver.  `resolve` gives the class a name
    /// written at an offset of `content` names.
    pub(crate) fn confirmed_form_type(
        &self,
        site: &OptionsSite<'_, '_>,
        content: &str,
        loaders: &crate::resolution::FileLoaders<'_>,
        resolve: &dyn Fn(&str, u32) -> String,
    ) -> Option<String> {
        let var_ctx = loaders.var_ctx_at(content, site.receiver.span().start.offset);
        let ty = crate::type_engine::variable::iterable_elements::resolve_expression_type(
            site.receiver,
            &var_ctx,
        )?;
        let confirmed = site.receivers.iter().any(|receiver| {
            crate::class_lookup::is_subtype_of_named(&ty, receiver, loaders.resolved_class_loader())
        });
        if !confirmed {
            return None;
        }
        Some(match site.form_type {
            FormTypeName::Written(name, offset) => {
                resolve(name, offset).trim_start_matches('\\').to_string()
            }
            FormTypeName::Fqn(fqn) => fqn.to_string(),
        })
    }

    /// The options of the form type whose options array holds the string
    /// `wanted` picks in `content`, with that string and the keys the array
    /// already has besides it.
    pub(crate) fn form_options_at(
        &self,
        content: &str,
        ctx: &crate::types::FileContext,
        wanted: impl Fn(&OptionString) -> bool,
    ) -> Option<(FormOptions, OptionString, Vec<String>)> {
        if !may_pass_form_options(content) {
            return None;
        }
        let loaders = self.file_loaders(ctx);
        let resolve = |name: &str, offset: u32| ctx.resolve_name_at(name, offset);
        let _parse_guard = crate::parser::with_parse_cache(content);
        let (form_type, string, others) = crate::parser::with_parsed_program(
            content,
            "symfony_form_option_at",
            |program, content| {
                options_sites(program).iter().find_map(|site| {
                    let string = *site.strings.iter().find(|string| wanted(string))?;
                    let form_type = self.confirmed_form_type(site, content, &loaders, &resolve)?;
                    let others = site
                        .strings
                        .iter()
                        .filter(|other| other.is_key && other.start != string.start)
                        .filter_map(|other| content.get(other.start as usize..other.end as usize))
                        .map(str::to_string)
                        .collect();
                    Some((form_type, string, others))
                })
            },
        )?;
        let options = self.form_type_options(&form_type, loaders.resolved_class_loader());
        Some((options, string, others))
    }

    /// Where the option of a form type an options array names at `offset`
    /// is declared.
    pub(crate) fn form_option_definition(
        &self,
        uri: &str,
        content: &str,
        offset: u32,
    ) -> Vec<tower_lsp_server::gen_lsp_types::Location> {
        if !may_pass_form_options(content) {
            return Vec::new();
        }
        let ctx = self.file_context(uri);
        let Some((options, string, _)) = self.form_options_at(content, &ctx, |string| {
            string.is_key && string.start <= offset && offset <= string.end
        }) else {
            return Vec::new();
        };
        let Some(name) = content.get(string.start as usize..string.end as usize) else {
            return Vec::new();
        };
        options
            .options
            .iter()
            .filter(|option| option.name == name)
            .filter_map(|option| {
                let declaring = self.get_file_content_arc(&option.uri)?;
                Some(tower_lsp_server::gen_lsp_types::Location {
                    uri: crate::util::parse_url(&option.uri)?,
                    range: crate::text_position::byte_range_to_lsp_range(
                        &declaring,
                        option.start as usize,
                        option.end as usize,
                    ),
                })
            })
            .collect()
    }

    /// Forget the installed packages' form classes, once they changed.
    pub(crate) fn forget_vendor_form_classes(&self) {
        self.symfony.vendor_form_classes.classes.write().clear();
        *self.symfony.vendor_form_classes.extensions.write() = None;
    }

    fn is_vendor_uri_of_project(&self, uri: &str) -> bool {
        self.workspace
            .vendor_uri_prefixes
            .lock()
            .iter()
            .any(|prefix| uri.starts_with(prefix.as_str()))
    }

    /// The form methods of the class `fqn`.
    fn form_class(&self, fqn: &str) -> Option<Arc<FormClass>> {
        let key = fqn.to_ascii_lowercase();
        if let Some(known) = self.symfony.vendor_form_classes.classes.read().get(&key) {
            return Some(Arc::clone(known));
        }
        let info = self.find_or_load_class(fqn)?;
        let uri = self.class_declaration_uri(fqn)?;
        let content = self.get_file_content_arc(&uri)?;
        let reading = read_form_class_in(&uri, &content, fqn)?;
        let vendor = self.is_vendor_uri_of_project(&uri);
        let class = Arc::new(FormClass {
            parent_class: info.parent_class.as_ref().map(|parent| parent.to_string()),
            uses_traits: !info.used_traits.is_empty(),
            uri,
            reading,
        });
        if vendor {
            self.symfony
                .vendor_form_classes
                .classes
                .write()
                .insert(key, Arc::clone(&class));
        }
        Some(class)
    }

    /// `fqn` and its parent classes, nearest first.  `None` when one of them
    /// cannot be read.
    fn form_class_chain(&self, fqn: &str) -> Option<Vec<Arc<FormClass>>> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut current = Some(fqn.to_string());
        while let Some(name) = current {
            if !seen.insert(name.to_ascii_lowercase()) {
                break;
            }
            let class = self.form_class(&name)?;
            current = class.parent_class.clone();
            chain.push(class);
        }
        Some(chain)
    }

    /// The type extensions of the installed packages: the classes named
    /// like an extension whose source declares the types it extends.
    fn vendor_form_type_extensions(&self) -> Arc<Vec<String>> {
        if let Some(known) = self.symfony.vendor_form_classes.extensions.read().as_ref() {
            return Arc::clone(known);
        }
        let prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let mut candidates: Vec<(String, String)> = self
            .symbols
            .fqn_uri_index
            .read()
            .iter()
            .filter(|(fqn, uri)| {
                fqn.len() > "Extension".len()
                    && fqn[fqn.len() - "Extension".len()..].eq_ignore_ascii_case("Extension")
                    && prefixes
                        .iter()
                        .any(|prefix| uri.starts_with(prefix.as_str()))
            })
            .map(|(fqn, uri)| (fqn.to_string(), uri.clone()))
            .collect();
        candidates.sort_unstable();
        let finder = memchr::memmem::Finder::new(b"getExtendedType");
        let extensions: Vec<String> = candidates
            .into_iter()
            .filter(|(_, uri)| {
                crate::util::uri_to_path(uri)
                    .and_then(|path| std::fs::read(path).ok())
                    .is_some_and(|content| finder.find(&content).is_some())
            })
            .map(|(fqn, _)| fqn)
            .collect();
        let extensions = Arc::new(extensions);
        *self.symfony.vendor_form_classes.extensions.write() = Some(Arc::clone(&extensions));
        extensions
    }

    /// Every type extension the project can use, with the types it
    /// extends, and whether all of them could be read.
    fn form_type_extensions(
        &self,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> (Vec<TypeExtension>, bool) {
        let mut names: Vec<String> = self
            .find_implementors(
                "FormTypeExtensionInterface",
                TYPE_EXTENSION,
                class_loader,
                false,
                false,
                true,
            )
            .iter()
            .map(|class| class.fqn().to_string())
            .collect();
        names.extend(self.vendor_form_type_extensions().iter().cloned());
        let mut complete = true;
        let mut extensions = Vec::new();
        for name in names {
            let Some(chain) = self.form_class_chain(&name) else {
                complete = false;
                continue;
            };
            let extended = chain
                .iter()
                .find_map(|class| class.reading.extended_types.clone());
            match extended {
                Some(ExtendedTypes::Types(types)) => {
                    extensions.push(TypeExtension { chain, types })
                }
                Some(ExtendedTypes::Unreadable) => complete = false,
                // An abstract base, which extends nothing until a subclass
                // says what.
                None => {}
            }
        }
        (extensions, complete)
    }

    /// Every option the form type `fqn` accepts.
    pub(crate) fn form_type_options(
        &self,
        fqn: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> FormOptions {
        let (extensions, complete) = self.form_type_extensions(class_loader);
        let mut options = FormOptions {
            options: Vec::new(),
            complete,
        };
        let mut visited = HashSet::new();
        self.collect_type_options(fqn, &extensions, &mut visited, &mut options);
        options
    }

    fn collect_type_options(
        &self,
        fqn: &str,
        extensions: &[TypeExtension],
        visited: &mut HashSet<String>,
        options: &mut FormOptions,
    ) {
        if !visited.insert(fqn.to_ascii_lowercase()) {
            return;
        }
        let Some(chain) = self.form_class_chain(fqn) else {
            options.complete = false;
            return;
        };
        match chain
            .iter()
            .find_map(|class| class.reading.get_parent.as_ref())
        {
            Some(ParentType::Type(parent)) => {
                self.collect_type_options(parent, extensions, visited, options);
            }
            Some(ParentType::Unreadable) => options.complete = false,
            Some(ParentType::Root) | None => {}
        }
        collect_configured_options(&chain, options);
        for extension in extensions {
            if extension
                .types
                .iter()
                .any(|extended| extended.eq_ignore_ascii_case(fqn))
            {
                collect_configured_options(&extension.chain, options);
            }
        }
    }
}

/// Collect the options the `configureOptions()` a class runs declares,
/// following `parent::configureOptions()` up its parent classes.
fn collect_configured_options(chain: &[Arc<FormClass>], options: &mut FormOptions) {
    for class in chain {
        let Some(configured) = &class.reading.configure else {
            // The method may come from a trait.
            if class.uses_traits {
                options.complete = false;
                return;
            }
            continue;
        };
        options.complete &= configured.complete;
        options.options.extend(
            configured
                .options
                .iter()
                .map(|(name, start, end)| FormOption {
                    name: name.clone(),
                    uri: class.uri.clone(),
                    start: *start,
                    end: *end,
                }),
        );
        if !configured.calls_parent {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_class<T>(php: &str, read: impl FnOnce(&Class<'_>) -> T) -> T {
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let class = find_class(program.statements.iter(), "", "App\\T").expect("class T");
        read(class)
    }

    fn class_of(expr: &Expression<'_>) -> Option<String> {
        crate::parser::class_fetch_identifier(expr).map(|identifier| {
            bytes_to_str(identifier.value())
                .trim_start_matches('\\')
                .to_string()
        })
    }

    fn option_names(configured: &ConfiguredOptions) -> Vec<&str> {
        configured
            .options
            .iter()
            .map(|(name, ..)| name.as_str())
            .collect()
    }

    #[test]
    fn options_are_read_from_every_declaring_call() {
        let php = "<?php\nnamespace App;\nclass T {\n    public function configureOptions(OptionsResolver $resolver): void {\n        parent::configureOptions($resolver);\n        $resolver->setDefaults(['a' => 1, 'b' => fn () => 2])->setAllowedTypes('a', 'int')->setDefault('c', null);\n        if (true) { $resolver->setRequired(['d']); } else { $resolver->setDefined('e'); }\n        $resolver->define('f')->default(1)->allowedTypes('int');\n        $resolver->setDefault('g', function (OptionsResolver $nested) { $nested->setDefaults(['inner' => 1]); });\n    }\n    public function getParent(): ?string { return \\Foo\\TextType::class; }\n}\n";
        let reading = parse_class(php, |class| read_form_class(class, &class_of));
        let configured = reading.configure.expect("configureOptions");
        assert_eq!(
            option_names(&configured),
            vec!["a", "b", "c", "d", "e", "f", "g"]
        );
        assert!(configured.calls_parent);
        assert!(configured.complete);
        assert_eq!(
            reading.get_parent,
            Some(ParentType::Type("Foo\\TextType".to_string()))
        );
        assert_eq!(reading.extended_types, None);
    }

    #[test]
    fn options_a_helper_or_a_variable_declares_make_the_set_incomplete() {
        for body in [
            "$this->configureCommon($resolver);",
            "$resolver->setDefaults($defaults);",
            "$resolver->setDefined(array_keys($x));",
            "$resolver->setDefaults([...$x]);",
        ] {
            let php = format!(
                "<?php\nnamespace App;\nclass T {{\n    public function configureOptions(OptionsResolver $resolver): void {{ {body} }}\n}}\n"
            );
            let reading = parse_class(&php, |class| read_form_class(class, &class_of));
            assert!(!reading.configure.unwrap().complete, "{body}");
        }
    }

    #[test]
    fn extended_types_and_root_parents_are_read() {
        let php = "<?php\nnamespace App;\nclass T {\n    public static function getExtendedTypes(): iterable { return [FormType::class, TextType::class]; }\n    public function getParent(): ?string { return null; }\n}\n";
        let reading = parse_class(php, |class| read_form_class(class, &class_of));
        assert_eq!(
            reading.extended_types,
            Some(ExtendedTypes::Types(vec![
                "FormType".to_string(),
                "TextType".to_string()
            ]))
        );
        assert_eq!(reading.get_parent, Some(ParentType::Root));

        let yields = "<?php\nnamespace App;\nclass T {\n    public static function getExtendedTypes(): iterable { yield FormType::class; }\n}\n";
        let reading = parse_class(yields, |class| read_form_class(class, &class_of));
        assert_eq!(
            reading.extended_types,
            Some(ExtendedTypes::Types(vec!["FormType".to_string()]))
        );
    }

    #[test]
    fn options_arrays_are_found_with_their_type() {
        let php = "<?php\n$builder->add('email', EmailType::class, ['required' => true, 'lab']);\n$builder->add('name', null, ['x' => 1]);\n$this->createForm(type: UserType::class, options: ['y' => 2]);\n$this->createFormBuilder(null, ['z' => 3]);\n$factory->create(UserType::class, null, $options);\n";
        let arena = mago_allocator::LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"test.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let found: Vec<(FormTypeName<'_>, Vec<(&str, bool)>)> = options_sites(program)
            .into_iter()
            .map(|site| {
                let strings = site
                    .strings
                    .iter()
                    .map(|s| (&php[s.start as usize..s.end as usize], s.is_key))
                    .collect();
                let form_type = match site.form_type {
                    FormTypeName::Written(name, _) => FormTypeName::Written(name, 0),
                    other => other,
                };
                (form_type, strings)
            })
            .collect();
        assert_eq!(
            found,
            vec![
                (
                    FormTypeName::Written("EmailType", 0),
                    vec![("required", true), ("lab", false)]
                ),
                (FormTypeName::Written("UserType", 0), vec![("y", true)]),
                (FormTypeName::Fqn(FORM_TYPE), vec![("z", true)]),
            ]
        );
    }
}
