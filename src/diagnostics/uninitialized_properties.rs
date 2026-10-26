//! Typed properties that nothing initializes.
//!
//! A typed property without a default value starts *uninitialized*, and
//! reading it before something writes it throws:
//!
//! ```php
//! final class User {
//!     private string $name;
//!     public function name(): string { return $this->name; }
//! }
//! (new User())->name(); // Error: must not be accessed before initialization
//! ```
//!
//! The check is on the declaration: a class property with a native type,
//! no default, and no promotion that nothing in the class body ever
//! writes is reported.  "Anything writes it" is deliberately broad:
//! an assignment from any method (a setter, a lazy `??=`, a named
//! constructor writing `$self->name`) counts.
//!
//! Only a property no code outside the class can write is checked: a
//! private one, a protected one in a class nothing can extend, and a
//! readonly one wherever only its declaring class may initialize it.  A
//! public property is routinely filled by whoever creates the object
//! (`$options = new Options(); $options->host = …`), which a look at the
//! class body cannot rule out; its reads are checked instead, by
//! [`super::uninitialized_reads`].
//!
//! Frameworks and ORMs hydrate properties by reflection, which this check
//! cannot see, so it stays quiet wherever that may be happening:
//!
//! - a property carrying an attribute, or an annotation-style docblock
//!   tag (`@ORM\Column`, `@Inject`, `@required`), is left alone;
//! - a class that writes a property by a dynamic name (`$this->$key`) or
//!   passes `$this` to a call may initialize anything;
//! - a class that runs an inherited constructor, or uses a trait, may
//!   have its properties written by code in another class body;
//! - a Doctrine entity mapped in XML or YAML is skipped, since nothing in
//!   the class says which of its fields Doctrine generates.
//!
//! A Doctrine mapping attribute is the exception to the attribute rule:
//! it says exactly which fields the ORM writes itself (a generated
//! identifier, a version field, a database-generated column, every field
//! of a read-only entity without a constructor), so any other mapped field
//! is still the class's to initialize.

use std::collections::HashSet;

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::argument::PartialArgument;
use mago_syntax::cst::argument::{NamedArgument, PositionalArgument};
use mago_syntax::cst::array::ArrayElement;
use mago_syntax::cst::attribute::{Attribute, AttributeList};
use mago_syntax::cst::call::StaticMethodCall;
use mago_syntax::cst::class_like::member::{ClassLikeMember, ClassLikeMemberSelector};
use mago_syntax::cst::class_like::property::{Property, PropertyItem};
use mago_syntax::cst::class_like::{AnonymousClass, Class};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::r#loop::foreach::{ForeachKeyValueTarget, ForeachValueTarget};
use mago_syntax::cst::modifier::Modifier;
use mago_syntax::cst::sequence::{Sequence, TokenSeparatedSequence};
use mago_syntax::cst::unary::UnaryPrefixOperator;
use mago_syntax::cst::{ModifierSequenceExt, Trivia};
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::docblock::get_docblock_text_for_node;
use crate::names::OwnedResolvedNames;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::types::{ClassInfo, MAX_INHERITANCE_DEPTH, PhpVersion};

use super::class_init_facts::is_this;
use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code used for a typed property nothing initializes.
pub(crate) const UNINITIALIZED_PROPERTY_CODE: &str = "uninitialized_property";

/// A typed property declaration that starts uninitialized and that no
/// framework fills.
#[derive(Clone)]
struct Candidate {
    /// The name without the `$`.
    name: String,
    /// Byte range of the `$name` variable in the declaration.
    start: u32,
    end: u32,
    is_readonly: bool,
    /// Whether only the class itself may write the property, so that the
    /// declaration is reported when the class never does.
    only_this_class_writes: bool,
    /// Whether code outside the class may read the property.
    is_public: bool,
}

/// What one class body declares and writes.
#[derive(Clone)]
pub(super) struct ClassBody {
    /// Left-brace offset, matching [`ClassInfo::start_offset`].
    start_offset: u32,
    candidates: Vec<Candidate>,
    /// Names of every property the body declares, promoted constructor
    /// parameters included.
    declared: HashSet<String>,
    /// Names of properties written through any receiver in the body.
    written: HashSet<String>,
    /// Whether the body declares its own `__construct`.
    has_constructor: bool,
    /// Whether the body calls `parent::__construct()`.
    calls_parent_constructor: bool,
    /// Whether the body uses a trait.
    uses_traits: bool,
    /// Whether something in the body may initialize properties this
    /// check cannot see: a write by dynamic name, or `$this` handed to a
    /// call.
    may_write_anything: bool,
}

#[derive(Default)]
struct FileBodies<'a> {
    trivia: &'a [Trivia<'a>],
    content: &'a str,
    /// Whether a `readonly` property may be initialized from a subclass
    /// (PHP 8.4 made `readonly` imply `protected(set)`; before that only
    /// the declaring class could).
    readonly_is_protected_set: bool,
    names: Option<&'a OwnedResolvedNames>,
    /// Class bodies still being walked, innermost last.  A write inside
    /// a nested anonymous class may target the outer class too, so it is
    /// recorded in every open body.
    open: Vec<ClassBody>,
    done: Vec<ClassBody>,
}

impl FileBodies<'_> {
    fn open_body(
        &mut self,
        left_brace: u32,
        is_final: bool,
        is_readonly_class: bool,
        attribute_lists: &Sequence<'_, AttributeList<'_>>,
        members: &Sequence<'_, ClassLikeMember<'_>>,
    ) {
        let mut body = ClassBody {
            start_offset: left_brace,
            candidates: Vec::new(),
            declared: HashSet::new(),
            written: HashSet::new(),
            has_constructor: false,
            calls_parent_constructor: false,
            uses_traits: false,
            may_write_anything: false,
        };
        for member in members.iter() {
            match member {
                ClassLikeMember::TraitUse(_) => body.uses_traits = true,
                ClassLikeMember::Method(method)
                    if bytes_to_str(method.name.value).eq_ignore_ascii_case("__construct") =>
                {
                    body.has_constructor = true;
                    for param in method.parameter_list.parameters.iter() {
                        if param.is_promoted_property() {
                            body.declared.insert(property_name(param.variable.name));
                        }
                    }
                }
                ClassLikeMember::Property(property) => {
                    for variable in property.variables() {
                        body.declared.insert(property_name(variable.name));
                    }
                }
                _ => {}
            }
        }
        // Doctrine fills every mapped property of a read-only entity that
        // has no constructor of its own.
        let doctrine_fills_all =
            !body.has_constructor && is_read_only_entity(self.names, attribute_lists);

        for member in members.iter() {
            // A hooked property may be virtual, with no backing value to
            // initialize.
            let ClassLikeMember::Property(Property::Plain(property)) = member else {
                continue;
            };
            if property.hint.is_none()
                || property.modifiers.contains_static()
                || filled_by_framework(self.names, &property.attribute_lists, doctrine_fills_all)
            {
                continue;
            }
            let is_readonly = is_readonly_class || property.modifiers.contains_readonly();
            let scope = write_scope(
                &property.modifiers,
                is_readonly,
                self.readonly_is_protected_set,
            );
            let only_this_class_writes = match scope {
                WriteScope::Private => true,
                WriteScope::Protected => is_final,
                WriteScope::Public => false,
            };
            let is_public = matches!(
                property.modifiers.get_first_read_visibility(),
                None | Some(Modifier::Public(_))
            );
            let tags = get_docblock_text_for_node(self.trivia, self.content, property)
                .map(DocTags::read)
                .unwrap_or_default();
            if tags.injected {
                continue;
            }
            let is_readonly = is_readonly || tags.readonly;
            for item in property.items.iter() {
                let PropertyItem::Abstract(item) = item else {
                    continue;
                };
                let span = item.variable.span();
                body.candidates.push(Candidate {
                    name: property_name(item.variable.name),
                    start: span.start.offset,
                    end: span.end.offset,
                    is_readonly,
                    only_this_class_writes,
                    is_public,
                });
            }
        }
        self.open.push(body);
    }

    fn close_body(&mut self) {
        if let Some(body) = self.open.pop() {
            self.done.push(body);
        }
    }

    fn record_write(&mut self, name: &str) {
        for body in &mut self.open {
            if !body.written.contains(name) {
                body.written.insert(name.to_string());
            }
        }
    }

    fn give_up(&mut self) {
        for body in &mut self.open {
            body.may_write_anything = true;
        }
    }
}

/// A property name without the `$` of its variable.
fn property_name(variable: &[u8]) -> String {
    let raw = bytes_to_str(variable);
    raw.strip_prefix('$').unwrap_or(raw).to_string()
}

/// Which scopes may write a property.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WriteScope {
    Private,
    Protected,
    Public,
}

/// The widest scope that may write a property declared with `modifiers`.
fn write_scope(
    modifiers: &Sequence<'_, Modifier<'_>>,
    is_readonly: bool,
    readonly_is_protected_set: bool,
) -> WriteScope {
    match modifiers.get_first_write_visibility() {
        Some(Modifier::PrivateSet(_)) => return WriteScope::Private,
        Some(Modifier::ProtectedSet(_)) => return WriteScope::Protected,
        Some(_) => return WriteScope::Public,
        None => {}
    }
    let read = match modifiers.get_first_read_visibility() {
        Some(Modifier::Private(_)) => WriteScope::Private,
        Some(Modifier::Protected(_)) => WriteScope::Protected,
        _ => WriteScope::Public,
    };
    if !is_readonly {
        return read;
    }
    let readonly = if readonly_is_protected_set {
        WriteScope::Protected
    } else {
        WriteScope::Private
    };
    read.min(readonly)
}

/// The short name of a Doctrine ORM mapping attribute (`Column`, `Id`,
/// …), or `None` for any other attribute.
fn doctrine_mapping<'n>(
    names: Option<&'n OwnedResolvedNames>,
    attribute: &Attribute<'_>,
) -> Option<&'n str> {
    let fqn = names?.get(attribute.name.span().start.offset)?;
    let fqn = fqn.strip_prefix('\\').unwrap_or(fqn);
    let prefix = fqn.get(..DOCTRINE_MAPPING.len())?;
    prefix
        .eq_ignore_ascii_case(DOCTRINE_MAPPING)
        .then(|| &fqn[DOCTRINE_MAPPING.len()..])
}

const DOCTRINE_MAPPING: &str = "Doctrine\\ORM\\Mapping\\";

/// The value of an attribute's argument, given by name or, when it has
/// one worth spelling out, by position.
fn attribute_argument<'x>(
    attribute: &'x Attribute<'x>,
    name: &str,
    position: Option<usize>,
) -> Option<&'x Expression<'x>> {
    let arguments = &attribute.argument_list.as_ref()?.arguments;
    let named = arguments.iter().find_map(|argument| match argument {
        PartialArgument::Named(named) if bytes_to_str(named.name.value) == name => {
            Some(named.value)
        }
        _ => None,
    });
    named.or_else(|| match arguments.iter().nth(position?) {
        Some(PartialArgument::Positional(positional)) => Some(positional.value),
        _ => None,
    })
}

/// The unquoted value of a string literal.
fn string_value<'x>(expr: &'x Expression<'x>) -> Option<&'x str> {
    match expr {
        Expression::Literal(Literal::String(string)) => string.value.map(bytes_to_str),
        _ => None,
    }
}

/// Whether the class attributes declare a read-only Doctrine entity,
/// `#[ORM\Entity(readOnly: true)]`.
fn is_read_only_entity(
    names: Option<&OwnedResolvedNames>,
    lists: &Sequence<'_, AttributeList<'_>>,
) -> bool {
    lists
        .iter()
        .flat_map(|list| list.attributes.iter())
        .filter(|attribute| {
            doctrine_mapping(names, attribute)
                .is_some_and(|name| name.eq_ignore_ascii_case("Entity"))
        })
        .any(|attribute| {
            matches!(
                attribute_argument(attribute, "readOnly", Some(1)),
                Some(Expression::Literal(Literal::True(_)))
            )
        })
}

/// Whether a framework writes the property its attributes describe.
///
/// A Doctrine mapping says exactly which columns the ORM writes on its
/// own: a generated identifier, a version field, a database-generated
/// column, and everything in a read-only entity without a constructor
/// (`doctrine_fills_all`).  Any other mapped column is the class's to
/// initialize.  An attribute from anywhere else (a serializer, a DI
/// container) may well mean the property is filled by reflection, so it
/// counts as written.  PHP's own attributes, in the global namespace,
/// say nothing either way.
fn filled_by_framework(
    names: Option<&OwnedResolvedNames>,
    lists: &Sequence<'_, AttributeList<'_>>,
    doctrine_fills_all: bool,
) -> bool {
    let mut doctrine_mapped = false;
    for attribute in lists.iter().flat_map(|list| list.attributes.iter()) {
        let Some(mapping) = doctrine_mapping(names, attribute) else {
            let fqn = names.and_then(|names| names.get(attribute.name.span().start.offset));
            if fqn.is_none_or(|fqn| fqn.trim_start_matches('\\').contains('\\')) {
                return true;
            }
            continue;
        };
        doctrine_mapped = true;
        let generated = if mapping.eq_ignore_ascii_case("GeneratedValue") {
            attribute_argument(attribute, "strategy", Some(0))
                .and_then(string_value)
                .is_none_or(|strategy| !strategy.eq_ignore_ascii_case("NONE"))
        } else if mapping.eq_ignore_ascii_case("Column") {
            attribute_argument(attribute, "generated", None).is_some_and(|value| {
                string_value(value).is_none_or(|v| !v.eq_ignore_ascii_case("NEVER"))
            })
        } else {
            mapping.eq_ignore_ascii_case("Version")
        };
        if generated {
            return true;
        }
    }
    doctrine_mapped && doctrine_fills_all
}

/// What a property's docblock tags say about its initialization.
#[derive(Default)]
struct DocTags {
    /// A tag marks the property for a framework to fill: a Doctrine-style
    /// annotation (`@ORM\Column`, `@Id`, `@Inject`) or the lowercase
    /// injection markers Symfony (`@required`) and Nette (`@inject`) read.
    injected: bool,
    /// `@readonly`, or its Psalm or PHPStan spelling.
    readonly: bool,
}

impl DocTags {
    fn read(docblock: &str) -> Self {
        let mut tags = DocTags::default();
        for tag in docblock.split('@').skip(1) {
            let name = tag
                .split(|c: char| !(c.is_alphanumeric() || c == '\\' || c == '_' || c == '-'))
                .next()
                .unwrap_or("");
            if name.starts_with(|c: char| c.is_ascii_uppercase() || c == '\\')
                || name.eq_ignore_ascii_case("inject")
                || name.eq_ignore_ascii_case("required")
            {
                tags.injected = true;
            } else if matches!(name, "readonly" | "psalm-readonly" | "phpstan-readonly") {
                tags.readonly = true;
            }
        }
        tags
    }
}

// ── AST walk ────────────────────────────────────────────────────────────────

struct BodyWalker;

impl<'ast, 'arena, 'a> mago_syntax::walker::Walker<'ast, 'arena, FileBodies<'a>> for BodyWalker {
    fn walk_in_class(&self, node: &'ast Class<'arena>, ctx: &mut FileBodies<'a>) {
        ctx.open_body(
            node.left_brace.start.offset,
            node.modifiers.contains_final(),
            node.modifiers.contains_readonly(),
            &node.attribute_lists,
            &node.members,
        );
    }

    fn walk_out_class(&self, _node: &'ast Class<'arena>, ctx: &mut FileBodies<'a>) {
        ctx.close_body();
    }

    fn walk_in_anonymous_class(
        &self,
        node: &'ast AnonymousClass<'arena>,
        ctx: &mut FileBodies<'a>,
    ) {
        // Nothing can extend an anonymous class.
        ctx.open_body(
            node.left_brace.start.offset,
            true,
            node.modifiers.contains_readonly(),
            &node.attribute_lists,
            &node.members,
        );
    }

    fn walk_out_anonymous_class(
        &self,
        _node: &'ast AnonymousClass<'arena>,
        ctx: &mut FileBodies<'a>,
    ) {
        ctx.close_body();
    }

    fn walk_in_expression(&self, node: &'ast Expression<'arena>, ctx: &mut FileBodies<'a>) {
        if ctx.open.is_empty() {
            return;
        }
        match node {
            Expression::Assignment(assign) => record_target(assign.lhs, ctx),
            Expression::UnaryPrefix(unary) => match unary.operator {
                UnaryPrefixOperator::PreIncrement(_)
                | UnaryPrefixOperator::PreDecrement(_)
                | UnaryPrefixOperator::Reference(_) => record_target(unary.operand, ctx),
                _ => {}
            },
            Expression::UnaryPostfix(unary) => record_target(unary.operand, ctx),
            _ => {}
        }
    }

    fn walk_in_foreach_value_target(
        &self,
        node: &'ast ForeachValueTarget<'arena>,
        ctx: &mut FileBodies<'a>,
    ) {
        record_target(node.value, ctx);
    }

    fn walk_in_foreach_key_value_target(
        &self,
        node: &'ast ForeachKeyValueTarget<'arena>,
        ctx: &mut FileBodies<'a>,
    ) {
        record_target(node.key, ctx);
        record_target(node.value, ctx);
    }

    fn walk_in_positional_argument(
        &self,
        node: &'ast PositionalArgument<'arena>,
        ctx: &mut FileBodies<'a>,
    ) {
        if is_this(node.value) {
            ctx.give_up();
        }
    }

    fn walk_in_named_argument(&self, node: &'ast NamedArgument<'arena>, ctx: &mut FileBodies<'a>) {
        if is_this(node.value) {
            ctx.give_up();
        }
    }

    fn walk_in_static_method_call(
        &self,
        node: &'ast StaticMethodCall<'arena>,
        ctx: &mut FileBodies<'a>,
    ) {
        if let Expression::Parent(_) = node.class
            && let ClassLikeMemberSelector::Identifier(ident) = &node.method
            && bytes_to_str(ident.value).eq_ignore_ascii_case("__construct")
            && let Some(body) = ctx.open.last_mut()
        {
            body.calls_parent_constructor = true;
        }
    }
}

/// Record the property a write in `expr` targets.  A write through an
/// array offset or a destructuring pattern counts as a write to the
/// property underneath.
fn record_target(expr: &Expression<'_>, ctx: &mut FileBodies<'_>) {
    match expr {
        Expression::Access(Access::Property(access)) => match &access.property {
            ClassLikeMemberSelector::Identifier(ident) => {
                ctx.record_write(bytes_to_str(ident.value))
            }
            _ => ctx.give_up(),
        },
        Expression::ArrayAccess(access) => record_target(access.array, ctx),
        Expression::ArrayAppend(append) => record_target(append.array, ctx),
        Expression::Array(array) => record_destructured(&array.elements, ctx),
        Expression::LegacyArray(array) => record_destructured(&array.elements, ctx),
        Expression::List(list) => record_destructured(&list.elements, ctx),
        Expression::Parenthesized(inner) => record_target(inner.expression, ctx),
        Expression::UnaryPrefix(unary) if unary.operator.is_reference() => {
            record_target(unary.operand, ctx)
        }
        _ => {}
    }
}

fn record_destructured(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut FileBodies<'_>,
) {
    for element in elements.iter() {
        let value = match element {
            ArrayElement::KeyValue(entry) => entry.value,
            ArrayElement::Value(entry) => entry.value,
            ArrayElement::Variadic(entry) => entry.value,
            ArrayElement::Missing(_) => continue,
        };
        record_target(value, ctx);
    }
}

/// Whether a class up the parent chain declares a constructor, or the
/// chain cannot be followed to its end.
fn inherits_constructor(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    let mut next = class.parent_class;
    let mut depth = 0u32;
    while let Some(parent_name) = next {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            return true;
        }
        let Some(parent) = class_loader(Fqn::new(&parent_name)) else {
            return true;
        };
        // A trait may bring the constructor, or write the property.
        if !parent.used_traits.is_empty()
            || parent
                .get_method("__construct")
                .is_some_and(|m| !m.is_virtual)
        {
            return true;
        }
        next = parent.parent_class;
    }
    false
}

/// What every class body in a parsed file declares and writes.
pub(super) fn class_bodies<'a>(
    program: &mago_syntax::cst::Program<'_>,
    content: &'a str,
    names: Option<&'a OwnedResolvedNames>,
    readonly_is_protected_set: bool,
) -> Vec<ClassBody> {
    let mut bodies = FileBodies {
        trivia: program.trivia.as_slice(),
        content,
        readonly_is_protected_set,
        names,
        ..FileBodies::default()
    };
    let walker = BodyWalker;
    for statement in program.statements.iter() {
        mago_syntax::walker::Walker::walk_statement(&walker, statement, &mut bodies);
    }
    bodies.done
}

impl ClassBody {
    /// Whether the body may write properties in ways this check cannot
    /// follow.
    pub(super) fn is_opaque(&self) -> bool {
        self.may_write_anything || self.uses_traits
    }

    pub(super) fn start_offset(&self) -> u32 {
        self.start_offset
    }

    pub(super) fn declares(&self, name: &str) -> bool {
        self.declared.contains(name)
    }

    pub(super) fn writes(&self, name: &str) -> bool {
        self.written.contains(name)
    }

    /// Whether `name` is a public property the body declares typed,
    /// without a default, and that no framework fills.
    pub(super) fn starts_uninitialized_in_public(&self, name: &str) -> bool {
        self.candidates
            .iter()
            .any(|candidate| candidate.is_public && candidate.name == name)
    }

    /// The candidates reported on the declaration: the ones only the
    /// class may write and that it never does.
    fn never_initialized(&self) -> impl Iterator<Item = &Candidate> {
        self.candidates.iter().filter(|candidate| {
            candidate.only_this_class_writes && !self.written.contains(&candidate.name)
        })
    }
}

// ── Main diagnostic collection ──────────────────────────────────────────────

impl Backend {
    /// Collect uninitialized-property diagnostics for a single file: the
    /// declarations nothing initializes, and the reads of a fresh
    /// object's property before anything writes it.
    pub(crate) fn collect_uninitialized_property_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let _parse_guard = with_parse_cache(content);
        let readonly_is_protected_set = self.php_version() >= PhpVersion::new(8, 4);
        let names = ctx.file.resolved_names.as_deref();

        let (bodies, reads) =
            with_parsed_program(content, "uninitialized_properties", |program, content| {
                let bodies = class_bodies(program, content, names, readonly_is_protected_set);
                let reads = super::uninitialized_reads::fresh_object_reads(program, content, names);
                (bodies, reads)
            });

        self.report_never_initialized(ctx, uri, content, &bodies, out);
        if !reads.is_empty() {
            self.report_uninitialized_reads(ctx, uri, content, &bodies, reads, out);
        }
    }

    fn report_never_initialized(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        bodies: &[ClassBody],
        out: &mut Vec<Diagnostic>,
    ) {
        let mut reported = bodies
            .iter()
            .filter(|body| !body.is_opaque() && body.never_initialized().next().is_some())
            .peekable();
        if reported.peek().is_none() {
            return;
        }

        let class_loaders = self.resolved_class_loaders(&ctx.file);
        for body in reported {
            let Some(class) = ctx
                .file
                .classes
                .iter()
                .find(|class| class.start_offset == body.start_offset)
            else {
                continue;
            };
            // An entity mapped in XML or YAML has its generated fields
            // written by Doctrine, and nothing in the class says which.
            if self.is_mapped_outside_the_class(class) {
                continue;
            }
            let runs_parent_constructor = !body.has_constructor || body.calls_parent_constructor;
            if runs_parent_constructor
                && inherits_constructor(class, class_loaders.at(body.start_offset))
            {
                continue;
            }

            for candidate in body.never_initialized() {
                if !super::incremental::reports_site(candidate.start) {
                    continue;
                }
                let owner = if class.name.starts_with("__anonymous@") {
                    format!("${}", candidate.name)
                } else {
                    format!("{}::${}", class.fqn(), candidate.name)
                };
                let message = if candidate.is_readonly {
                    format!(
                        "Readonly property {} is never initialized; assign it in the constructor",
                        owner
                    )
                } else {
                    format!(
                        "Property {} is never initialized; give it a default value or assign it in the constructor",
                        owner
                    )
                };
                let Some(range) = self.offset_range_to_lsp_range(
                    uri,
                    content,
                    candidate.start as usize,
                    candidate.end as usize,
                ) else {
                    continue;
                };
                out.push(make_diagnostic(
                    range,
                    DiagnosticSeverity::Warning,
                    UNINITIALIZED_PROPERTY_CODE,
                    message,
                ));
            }
        }
    }

    /// Whether Doctrine maps `class` in XML or YAML, where nothing in the
    /// class says which of its fields the ORM generates.
    pub(super) fn is_mapped_outside_the_class(&self, class: &ClassInfo) -> bool {
        self.is_doctrine_project()
            && !self
                .framework_doctrine_repository_fqns_for_entity(&class.fqn())
                .is_empty()
    }
}
