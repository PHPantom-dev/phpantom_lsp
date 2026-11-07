//! Private members nothing uses.
//!
//! A private member is reachable only from its own class body, so the
//! body alone decides whether anything uses it:
//!
//! ```php
//! final class Invoice
//! {
//!     private const LEGACY = 'x';     // never read
//!     private string $legacyNumber;   // never read nor written
//!     private ?string $note = null;   // written, never read
//!     private array $lines = [];      // read, never written
//!
//!     private function recalculate(): void {} // never called
//! }
//! ```
//!
//! A method, a constant, and a property neither read nor written are dead
//! code, reported dimmed with [`DiagnosticTag::Unnecessary`] and offered
//! for removal.  A property the class writes but never reads is dead too,
//! though removing it means removing its writes.  A property the class
//! reads but never writes holds its default forever; a typed one without
//! a default is `uninitialized_property`'s to report instead.
//!
//! "Uses" is read broadly, so that a member reported is one nothing could
//! be using:
//!
//! - an access names the member through any receiver (`$this->x`,
//!   `$other->x`, `self::X`, `static::x()`), and any class body nested in
//!   the class counts as the class;
//! - a string literal spelling the member's name may be a callable
//!   (`[$this, 'handle']`, `'self::handle'`) or a property name handed to
//!   a framework, so it uses the member;
//! - an access by dynamic name (`$this->$name`, `$this->{$name}()`, a
//!   callable array with a computed method) may reach any member of its
//!   kind, and `$this` handed to a call (`get_object_vars($this)`, a
//!   serializer), cast to an array, or iterated may read and write any
//!   property;
//! - a trait the class uses runs in the class's scope, so a name its
//!   source spells uses the member, and a trait that cannot be read
//!   leaves the class unjudged;
//! - a member carrying an attribute from outside the global namespace,
//!   or an annotation-style docblock tag, is left alone, since
//!   frameworks read and write those by reflection (Doctrine's mapped
//!   fields, Gedmo's, Symfony's `#[Required]`), and so is a property a
//!   Doctrine XML or YAML mapping file maps;
//! - a property passed as an argument may be passed by reference, so it
//!   counts as written.
//!
//! Magic methods (`__construct`, `__get`, …) are called by PHP itself,
//! and traits are skipped: their private members belong to every class
//! that uses them.

use std::collections::{HashMap, HashSet};

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::argument::{Argument, ArgumentList};
use mago_syntax::cst::array::ArrayElement;
use mago_syntax::cst::assignment::AssignmentOperator;
use mago_syntax::cst::attribute::AttributeList;
use mago_syntax::cst::call::Call;
use mago_syntax::cst::class_like::member::{
    ClassLikeConstantSelector, ClassLikeMember, ClassLikeMemberSelector,
};
use mago_syntax::cst::class_like::property::{Property, PropertyItem};
use mago_syntax::cst::class_like::{AnonymousClass, Class, Enum};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::literal::Literal;
use mago_syntax::cst::r#loop::foreach::{Foreach, ForeachKeyValueTarget, ForeachValueTarget};
use mago_syntax::cst::modifier::Modifier;
use mago_syntax::cst::partial_application::PartialApplication;
use mago_syntax::cst::sequence::{Sequence, TokenSeparatedSequence};
use mago_syntax::cst::type_hint::Hint;
use mago_syntax::cst::unary::UnaryPrefixOperator;
use mago_syntax::cst::unset::Unset;
use mago_syntax::cst::variable::Variable;
use mago_syntax::cst::{ModifierSequenceExt, Trivia};
use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::Fqn;
use crate::names::OwnedResolvedNames;
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::symbol_map::docblock::get_docblock_text_with_offset;

use super::class_init_facts::is_this;
use super::helpers::{FileDiagnosticContext, make_tagged_diagnostic};

/// A private property nothing reads.
pub(crate) const UNUSED_PROPERTY_CODE: &str = "unused_property";
/// A private property nothing writes, which holds its default forever.
pub(crate) const UNWRITTEN_PROPERTY_CODE: &str = "unwritten_property";
/// A private method nothing calls.
pub(crate) const UNUSED_METHOD_CODE: &str = "unused_method";
/// A private constant nothing reads.
pub(crate) const UNUSED_CONSTANT_CODE: &str = "unused_constant";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MemberKind {
    Property,
    Method,
    Constant,
}

/// How a member goes unused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Usage {
    /// Nothing reads or writes it.
    Unused,
    /// A property the class writes but never reads.
    OnlyWritten,
    /// A property the class reads but never writes.
    OnlyRead,
}

/// A private member its class does not use.
#[derive(Debug)]
pub(crate) struct UnusedMember {
    pub(crate) kind: MemberKind,
    pub(crate) usage: Usage,
    /// The member as a message names it: `Foo::$bar`, `Foo::bar()`,
    /// `Foo::BAR`, without the class in an anonymous class.
    pub(crate) label: String,
    /// Byte range of the member's name in its declaration.
    pub(crate) start: u32,
    pub(crate) end: u32,
    /// Byte range that removes the declaration, its docblock and
    /// attributes included, when removing it leaves working code.
    pub(crate) removal: Option<(u32, u32)>,
}

impl UnusedMember {
    pub(crate) fn code(&self) -> &'static str {
        match (self.kind, self.usage) {
            (MemberKind::Property, Usage::OnlyRead) => UNWRITTEN_PROPERTY_CODE,
            (MemberKind::Property, _) => UNUSED_PROPERTY_CODE,
            (MemberKind::Method, _) => UNUSED_METHOD_CODE,
            (MemberKind::Constant, _) => UNUSED_CONSTANT_CODE,
        }
    }

    fn message(&self) -> String {
        let what = match self.kind {
            MemberKind::Property => "Property",
            MemberKind::Method => "Method",
            MemberKind::Constant => "Constant",
        };
        match self.usage {
            Usage::Unused => format!("{what} {} is unused", self.label),
            Usage::OnlyWritten => format!("{what} {} is never read, only written", self.label),
            Usage::OnlyRead => format!("{what} {} is never written, only read", self.label),
        }
    }
}

/// A private member a class body declares, and what the body does with it.
struct Declared {
    kind: MemberKind,
    /// The name as declared, without a property's `$`.
    name: String,
    start: u32,
    end: u32,
    /// The range that removes the declaration; `None` for a promoted
    /// property, which only a rewrite of the constructor could remove.
    removal: Option<(u32, u32)>,
    is_hooked: bool,
    /// Whether the property has a value before anything writes it: a
    /// default, or a constructor promoting it.
    starts_initialized: bool,
    /// Whether `uninitialized_property` reports the property when nothing
    /// writes it: an instance property with a native type.
    checked_for_initialization: bool,
    /// Whether the declared type rules out an object, so that a write
    /// through an offset (`$this->items[] = $x`) stores into the property
    /// rather than calling `offsetSet()` on what it holds.
    holds_array: bool,
    /// A method or constant counts as used once read.
    read: bool,
    written: bool,
}

impl Declared {
    fn mark_all(&mut self) {
        self.read = true;
        self.written = true;
    }
}

/// One class body under the walk.
struct Body {
    /// Left-brace offset, matching [`ClassInfo::start_offset`].
    left_brace: u32,
    members: Vec<Declared>,
    /// The traits the body uses, fully qualified.
    traits: Vec<String>,
}

impl Body {
    fn members_of(&mut self, kind: MemberKind) -> impl Iterator<Item = &mut Declared> {
        self.members.iter_mut().filter(move |m| m.kind == kind)
    }
}

/// How a property is reached at one site.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The site reads the property.
    Read,
    /// An assignment stores into the property without reading it.
    Store,
    /// An assignment stores through an offset of the property, which
    /// reads it unless it holds an array.
    OffsetStore,
}

struct Scan<'a> {
    trivia: &'a [Trivia<'a>],
    content: &'a str,
    names: Option<&'a OwnedResolvedNames>,
    /// Class bodies still being walked, innermost last.  A use inside a
    /// nested class body counts for every body around it.
    open: Vec<Body>,
    done: Vec<Body>,
    /// How the assignment targets seen so far reach their property, by
    /// the offset of the property's name.
    stores: HashMap<u32, Reach>,
    /// Properties passed to a function named in the source.
    passed: Vec<PassedToFunction>,
}

/// A property passed to a function the source names, which writes it if
/// it takes the argument by reference.
struct PassedToFunction {
    function: String,
    /// Offset of the function's name.
    offset: u32,
    /// The argument's position, and its name when it is a named one.
    position: usize,
    name: Option<String>,
    property: String,
    /// The class bodies open around the call.
    bodies: Vec<u32>,
}

impl Scan<'_> {
    /// Whether an open body has a private member left to account for.
    fn tracking(&self) -> bool {
        self.open.iter().any(|body| !body.members.is_empty())
    }

    fn reach_at(&self, offset: u32) -> Reach {
        self.stores.get(&offset).copied().unwrap_or(Reach::Read)
    }

    fn read_property(&mut self, name: &str, reach: Reach) {
        for body in &mut self.open {
            for member in body.members_of(MemberKind::Property) {
                if member.name == name {
                    match reach {
                        Reach::Read => member.read = true,
                        Reach::Store => {}
                        Reach::OffsetStore => member.read |= !member.holds_array,
                    }
                }
            }
        }
    }

    fn write_property(&mut self, name: &str) {
        for body in &mut self.open {
            for member in body.members_of(MemberKind::Property) {
                if member.name == name {
                    member.written = true;
                }
            }
        }
    }

    fn use_method(&mut self, name: &str) {
        for body in &mut self.open {
            for member in body.members_of(MemberKind::Method) {
                if member.name.eq_ignore_ascii_case(name) {
                    member.read = true;
                }
            }
        }
    }

    fn use_constant(&mut self, name: &str) {
        for body in &mut self.open {
            for member in body.members_of(MemberKind::Constant) {
                if member.name == name {
                    member.read = true;
                }
            }
        }
    }

    /// A string literal naming a member: a callable, a property name a
    /// framework is handed, `constant('self::X')`.
    fn use_string(&mut self, value: &str) {
        if value.len() > 255 || value.is_empty() {
            return;
        }
        let after_scope = value.rsplit_once("::").map_or(value, |(_, member)| member);
        for body in &mut self.open {
            for member in &mut body.members {
                let names_it = match member.kind {
                    MemberKind::Property => {
                        let bare = after_scope.strip_prefix('$').unwrap_or(after_scope);
                        bare == member.name
                    }
                    MemberKind::Method => after_scope.eq_ignore_ascii_case(&member.name),
                    MemberKind::Constant => after_scope == member.name,
                };
                if names_it {
                    member.mark_all();
                }
            }
        }
    }

    /// Every member of `kind` may be reached by a name this walk cannot
    /// read.
    fn use_every(&mut self, kind: MemberKind) {
        for body in &mut self.open {
            for member in body.members_of(kind) {
                member.mark_all();
            }
        }
    }

    fn open_body(&mut self, left_brace: u32, members: &Sequence<'_, ClassLikeMember<'_>>) {
        let mut body = Body {
            left_brace,
            members: Vec::new(),
            traits: Vec::new(),
        };
        for member in members.iter() {
            self.declare(member, &mut body);
        }
        self.open.push(body);
    }

    fn close_body(&mut self) {
        if let Some(body) = self.open.pop() {
            self.done.push(body);
        }
    }

    /// Whether a framework may read or write `node` by reflection: it
    /// carries an attribute from outside the global namespace, or an
    /// annotation-style docblock tag.
    fn framework_managed(
        &self,
        attribute_lists: &Sequence<'_, AttributeList<'_>>,
        node: &impl HasSpan,
    ) -> bool {
        has_framework_attribute(self.names, attribute_lists)
            || get_docblock_text_with_offset(self.trivia, self.content, node).is_some_and(
                |(docblock, _)| super::uninitialized_properties::marks_injected(docblock),
            )
    }

    /// Where a declaration starts once its docblock is counted in.
    fn declaration_start(&self, node: &impl HasSpan) -> u32 {
        get_docblock_text_with_offset(self.trivia, self.content, node)
            .map_or(node.span().start.offset, |(_, offset)| offset)
    }

    fn declare(&self, member: &ClassLikeMember<'_>, body: &mut Body) {
        match member {
            ClassLikeMember::TraitUse(trait_use) => {
                for name in trait_use.trait_names.iter() {
                    let resolved = self
                        .names
                        .and_then(|names| names.get(name.span().start.offset))
                        .unwrap_or_else(|| bytes_to_str(name.value()));
                    body.traits
                        .push(resolved.trim_start_matches('\\').to_string());
                }
            }
            ClassLikeMember::Method(method) => {
                let name = bytes_to_str(method.name.value);
                if name.starts_with("__") {
                    if name.eq_ignore_ascii_case("__construct") {
                        self.declare_promoted(method, body);
                    }
                    return;
                }
                if !is_private(&method.modifiers)
                    || self.framework_managed(&method.attribute_lists, method)
                {
                    return;
                }
                let span = method.span();
                let start = self.declaration_start(method);
                body.members.push(Declared {
                    kind: MemberKind::Method,
                    name: name.to_string(),
                    start: method.name.span.start.offset,
                    end: method.name.span.end.offset,
                    removal: Some(line_removal(self.content, start, span.end.offset)),
                    is_hooked: false,
                    starts_initialized: false,
                    checked_for_initialization: false,
                    holds_array: false,
                    read: false,
                    written: false,
                });
            }
            ClassLikeMember::Constant(constant) => {
                if !is_private(&constant.modifiers)
                    || self.framework_managed(&constant.attribute_lists, constant)
                {
                    return;
                }
                let whole = (self.declaration_start(constant), constant.span().end.offset);
                let items: Vec<(u32, u32)> = constant
                    .items
                    .iter()
                    .map(|item| (item.span().start.offset, item.span().end.offset))
                    .collect();
                for (index, item) in constant.items.iter().enumerate() {
                    body.members.push(Declared {
                        kind: MemberKind::Constant,
                        name: bytes_to_str(item.name.value).to_string(),
                        start: item.name.span.start.offset,
                        end: item.name.span.end.offset,
                        removal: Some(item_removal(self.content, whole, &items, index)),
                        is_hooked: false,
                        starts_initialized: false,
                        checked_for_initialization: false,
                        holds_array: false,
                        read: false,
                        written: false,
                    });
                }
            }
            ClassLikeMember::Property(property) => {
                let (attribute_lists, modifiers, hint, is_hooked) = match property {
                    Property::Plain(plain) => {
                        (&plain.attribute_lists, &plain.modifiers, &plain.hint, false)
                    }
                    Property::Hooked(hooked) => (
                        &hooked.attribute_lists,
                        &hooked.modifiers,
                        &hooked.hint,
                        true,
                    ),
                };
                if !is_private(modifiers) || self.framework_managed(attribute_lists, property) {
                    return;
                }
                let is_static = modifiers.contains_static();
                let holds_array = hint.as_ref().is_none_or(hint_holds_array);
                let whole = (self.declaration_start(property), property.span().end.offset);
                let items: Vec<&PropertyItem<'_>> = match property {
                    Property::Plain(plain) => plain.items.iter().collect(),
                    Property::Hooked(hooked) => vec![&hooked.item],
                };
                let item_spans: Vec<(u32, u32)> = items
                    .iter()
                    .map(|item| (item.span().start.offset, item.span().end.offset))
                    .collect();
                for (index, item) in items.iter().enumerate() {
                    let (variable, has_default) = match item {
                        PropertyItem::Abstract(item) => (&item.variable, false),
                        PropertyItem::Concrete(item) => (&item.variable, true),
                    };
                    let span = variable.span();
                    body.members.push(Declared {
                        kind: MemberKind::Property,
                        name: property_name(variable.name),
                        start: span.start.offset,
                        end: span.end.offset,
                        removal: Some(item_removal(self.content, whole, &item_spans, index)),
                        is_hooked,
                        starts_initialized: has_default,
                        checked_for_initialization: hint.is_some() && !is_static,
                        holds_array,
                        read: false,
                        written: false,
                    });
                }
            }
            _ => {}
        }
    }

    /// The private properties a constructor promotes, which the
    /// constructor itself writes.
    fn declare_promoted(
        &self,
        method: &mago_syntax::cst::class_like::method::Method<'_>,
        body: &mut Body,
    ) {
        for param in method.parameter_list.parameters.iter() {
            // A parameter taken by reference binds the property to the
            // caller's variable, which reads what the class writes.
            if !param.is_promoted_property()
                || param.ampersand.is_some()
                || !is_private(&param.modifiers)
                || self.framework_managed(&param.attribute_lists, param)
            {
                continue;
            }
            let span = param.variable.span();
            body.members.push(Declared {
                kind: MemberKind::Property,
                name: property_name(param.variable.name),
                start: span.start.offset,
                end: span.end.offset,
                removal: None,
                is_hooked: param.hooks.is_some(),
                starts_initialized: true,
                checked_for_initialization: true,
                holds_array: param.hint.as_ref().is_none_or(hint_holds_array),
                read: false,
                written: true,
            });
        }
    }
}

/// Whether the read visibility of a member is `private`.
fn is_private(modifiers: &Sequence<'_, Modifier<'_>>) -> bool {
    matches!(
        modifiers.get_first_read_visibility(),
        Some(Modifier::Private(_))
    )
}

/// Whether a framework attribute marks the declaration: any attribute
/// outside the global namespace (PHP's own, `#[\Override]` and
/// `#[\Deprecated]`, say nothing about who uses a member), or one whose
/// name does not resolve.
fn has_framework_attribute(
    names: Option<&OwnedResolvedNames>,
    lists: &Sequence<'_, AttributeList<'_>>,
) -> bool {
    lists
        .iter()
        .flat_map(|list| list.attributes.iter())
        .any(|attribute| {
            let fqn = names.and_then(|names| names.get(attribute.name.span().start.offset));
            fqn.is_none_or(|fqn| fqn.trim_start_matches('\\').contains('\\'))
        })
}

/// Whether a declared type holds only arrays (or nothing an offset write
/// could call `offsetSet()` on).
fn hint_holds_array(hint: &Hint<'_>) -> bool {
    match hint {
        Hint::Array(_) | Hint::Null(_) => true,
        Hint::Nullable(nullable) => hint_holds_array(nullable.hint),
        Hint::Union(union) => hint_holds_array(union.left) && hint_holds_array(union.right),
        Hint::Parenthesized(inner) => hint_holds_array(inner.hint),
        _ => false,
    }
}

fn property_name(variable: &[u8]) -> String {
    let raw = bytes_to_str(variable);
    raw.strip_prefix('$').unwrap_or(raw).to_string()
}

/// The range that removes `start..end` with the lines it alone occupies,
/// and a blank line above it when one would otherwise be left doubled or
/// hanging before the closing brace.
fn line_removal(content: &str, start: u32, end: u32) -> (u32, u32) {
    let bytes = content.as_bytes();
    let (mut from, mut to) = (start as usize, (end as usize).min(bytes.len()));

    let line_start = content[..from].rfind('\n').map_or(0, |i| i + 1);
    if content[line_start..from]
        .trim_matches([' ', '\t'])
        .is_empty()
    {
        from = line_start;
    }
    let rest = &content[to..];
    let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
    if rest[..line_end].trim().is_empty() {
        to += line_end;
    } else {
        return (from as u32, to as u32);
    }
    if from != line_start {
        return (from as u32, to as u32);
    }

    // A blank line above, and below either another blank line or the
    // end of the body: drop it with the declaration.
    let above_end = from;
    let above_start = content[..above_end.saturating_sub(1)]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let above_is_blank = above_end > 0 && content[above_start..above_end].trim().is_empty();
    let below = &content[to..];
    let next_line = &below[..below.find('\n').map_or(below.len(), |i| i + 1)];
    let below_closes = next_line.trim().is_empty() || next_line.trim_start().starts_with('}');
    if above_is_blank && below_closes && above_start < above_end {
        from = above_start;
    }
    (from as u32, to as u32)
}

/// The range that removes the item at `index` of a declaration listing
/// `items`: the whole declaration when it is the only one, else the item
/// and the comma that separates it from a neighbour.
fn item_removal(
    content: &str,
    whole: (u32, u32),
    items: &[(u32, u32)],
    index: usize,
) -> (u32, u32) {
    if items.len() <= 1 {
        return line_removal(content, whole.0, whole.1);
    }
    let (start, end) = items[index];
    match items.get(index + 1) {
        Some(&(next, _)) => (start, next),
        None => (items[index - 1].1, end),
    }
}

// ── AST walk ────────────────────────────────────────────────────────────────

struct UsageWalker;

impl<'ast, 'arena, 'a> mago_syntax::walker::Walker<'ast, 'arena, Scan<'a>> for UsageWalker {
    fn walk_in_class(&self, node: &'ast Class<'arena>, ctx: &mut Scan<'a>) {
        ctx.open_body(node.left_brace.start.offset, &node.members);
    }

    fn walk_out_class(&self, _node: &'ast Class<'arena>, ctx: &mut Scan<'a>) {
        ctx.close_body();
    }

    fn walk_in_anonymous_class(&self, node: &'ast AnonymousClass<'arena>, ctx: &mut Scan<'a>) {
        ctx.open_body(node.left_brace.start.offset, &node.members);
    }

    fn walk_out_anonymous_class(&self, _node: &'ast AnonymousClass<'arena>, ctx: &mut Scan<'a>) {
        ctx.close_body();
    }

    fn walk_in_enum(&self, node: &'ast Enum<'arena>, ctx: &mut Scan<'a>) {
        ctx.open_body(node.left_brace.start.offset, &node.members);
    }

    fn walk_out_enum(&self, _node: &'ast Enum<'arena>, ctx: &mut Scan<'a>) {
        ctx.close_body();
    }

    fn walk_in_expression(&self, node: &'ast Expression<'arena>, ctx: &mut Scan<'a>) {
        if !ctx.tracking() {
            return;
        }
        match node {
            Expression::Assignment(assign) => {
                // `$this->p = &$x` binds the property to storage something
                // else reads, so a write through it is not a dead store.
                let binds_reference = matches!(
                    assign.rhs,
                    Expression::UnaryPrefix(unary) if unary.operator.is_reference()
                );
                let stores =
                    matches!(assign.operator, AssignmentOperator::Assign(_)) && !binds_reference;
                mark_target(assign.lhs, stores, ctx);
            }
            Expression::UnaryPrefix(unary) => match unary.operator {
                UnaryPrefixOperator::PreIncrement(_)
                | UnaryPrefixOperator::PreDecrement(_)
                | UnaryPrefixOperator::Reference(_) => mark_target(unary.operand, false, ctx),
                UnaryPrefixOperator::ArrayCast(..) if is_this(unary.operand) => {
                    ctx.use_every(MemberKind::Property)
                }
                _ => {}
            },
            Expression::UnaryPostfix(unary) => mark_target(unary.operand, false, ctx),
            Expression::Access(access) => match access {
                Access::Property(access) => property_selector(&access.property, ctx),
                Access::NullSafeProperty(access) => property_selector(&access.property, ctx),
                Access::StaticProperty(access) => match &access.property {
                    Variable::Direct(variable) => {
                        let reach = ctx.reach_at(variable.span.start.offset);
                        ctx.read_property(&property_name(variable.name), reach);
                    }
                    _ => ctx.use_every(MemberKind::Property),
                },
                Access::ClassConstant(access) => match &access.constant {
                    ClassLikeConstantSelector::Identifier(ident) => {
                        ctx.use_constant(bytes_to_str(ident.value))
                    }
                    ClassLikeConstantSelector::Expression(_) => ctx.use_every(MemberKind::Constant),
                    ClassLikeConstantSelector::Missing(_) => {}
                },
            },
            Expression::Call(call) => match call {
                Call::Method(call) => {
                    method_selector(&call.method, ctx);
                    arguments(&call.argument_list, None, ctx);
                }
                Call::NullSafeMethod(call) => {
                    method_selector(&call.method, ctx);
                    arguments(&call.argument_list, None, ctx);
                }
                Call::StaticMethod(call) => {
                    method_selector(&call.method, ctx);
                    arguments(&call.argument_list, None, ctx);
                }
                Call::Function(call) => {
                    let function = match call.function {
                        Expression::Identifier(name) => {
                            Some((bytes_to_str(name.value()), name.span().start.offset))
                        }
                        _ => None,
                    };
                    arguments(&call.argument_list, function, ctx);
                }
            },
            Expression::Instantiation(new) => {
                if let Some(list) = &new.argument_list {
                    arguments(list, None, ctx);
                }
            }
            Expression::PartialApplication(application) => match application {
                PartialApplication::Method(call) => method_selector(&call.method, ctx),
                PartialApplication::StaticMethod(call) => method_selector(&call.method, ctx),
                PartialApplication::Function(_) => {}
            },
            Expression::Literal(Literal::String(string)) => {
                if let Some(value) = string.value {
                    ctx.use_string(bytes_to_str(value));
                }
            }
            Expression::Array(array) => computed_callable(&array.elements, ctx),
            Expression::LegacyArray(array) => computed_callable(&array.elements, ctx),
            _ => {}
        }
    }

    fn walk_in_unset(&self, node: &'ast Unset<'arena>, ctx: &mut Scan<'a>) {
        if !ctx.tracking() {
            return;
        }
        for value in node.values.iter() {
            mark_target(value, true, ctx);
        }
    }

    fn walk_in_foreach(&self, node: &'ast Foreach<'arena>, ctx: &mut Scan<'a>) {
        if is_this(node.expression) {
            ctx.use_every(MemberKind::Property);
        }
    }

    fn walk_in_foreach_value_target(
        &self,
        node: &'ast ForeachValueTarget<'arena>,
        ctx: &mut Scan<'a>,
    ) {
        mark_target(node.value, true, ctx);
    }

    fn walk_in_foreach_key_value_target(
        &self,
        node: &'ast ForeachKeyValueTarget<'arena>,
        ctx: &mut Scan<'a>,
    ) {
        mark_target(node.key, true, ctx);
        mark_target(node.value, true, ctx);
    }
}

fn property_selector(selector: &ClassLikeMemberSelector<'_>, ctx: &mut Scan<'_>) {
    match selector {
        ClassLikeMemberSelector::Identifier(ident) => {
            let reach = ctx.reach_at(ident.span.start.offset);
            ctx.read_property(bytes_to_str(ident.value), reach);
        }
        ClassLikeMemberSelector::Variable(_) | ClassLikeMemberSelector::Expression(_) => {
            ctx.use_every(MemberKind::Property)
        }
        ClassLikeMemberSelector::Missing(_) => {}
    }
}

fn method_selector(selector: &ClassLikeMemberSelector<'_>, ctx: &mut Scan<'_>) {
    match selector {
        ClassLikeMemberSelector::Identifier(ident) => ctx.use_method(bytes_to_str(ident.value)),
        ClassLikeMemberSelector::Variable(_) | ClassLikeMemberSelector::Expression(_) => {
            ctx.use_every(MemberKind::Method)
        }
        ClassLikeMemberSelector::Missing(_) => {}
    }
}

/// The arguments of a call: `$this` handed over may have any property
/// read or written, and a property may be passed by reference.  Whether a
/// function named in the source takes it by reference is looked up once
/// the walk is done (see [`PassedToFunction`]); any other call is assumed
/// to.
fn arguments(list: &ArgumentList<'_>, function: Option<(&str, u32)>, ctx: &mut Scan<'_>) {
    for (index, argument) in list.arguments.iter().enumerate() {
        let (value, name) = match argument {
            Argument::Positional(argument) => (argument.value, None),
            Argument::Named(argument) => (argument.value, Some(bytes_to_str(argument.name.value))),
        };
        if is_this(value) {
            ctx.use_every(MemberKind::Property);
            continue;
        }
        let Some(property) = passed_property(value) else {
            continue;
        };
        match function {
            Some((function, offset)) => {
                let bodies = ctx.open.iter().map(|body| body.left_brace).collect();
                ctx.passed.push(PassedToFunction {
                    function: function.to_string(),
                    offset,
                    position: index,
                    name: name.map(str::to_string),
                    property,
                    bodies,
                });
            }
            None => mark_target(value, false, ctx),
        }
    }
}

/// The property an argument passes, itself or through an offset.
fn passed_property(mut value: &Expression<'_>) -> Option<String> {
    loop {
        match value {
            Expression::ArrayAccess(access) => value = access.array,
            Expression::Access(Access::Property(access)) => {
                return match &access.property {
                    ClassLikeMemberSelector::Identifier(ident) => {
                        Some(bytes_to_str(ident.value).to_string())
                    }
                    _ => None,
                };
            }
            Expression::Access(Access::StaticProperty(access)) => {
                return match &access.property {
                    Variable::Direct(variable) => Some(property_name(variable.name)),
                    _ => None,
                };
            }
            _ => return None,
        }
    }
}

/// A callable array whose method is computed (`[$this, $method]`) may
/// call any method.
fn computed_callable(elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>, ctx: &mut Scan<'_>) {
    if elements.len() != 2 {
        return;
    }
    let mut values = elements.iter().map(|element| match element {
        ArrayElement::Value(entry) => Some(entry.value),
        _ => None,
    });
    let (Some(Some(receiver)), Some(Some(method))) = (values.next(), values.next()) else {
        return;
    };
    let names_class = is_this(receiver)
        || matches!(
            receiver,
            Expression::Access(Access::ClassConstant(access))
                if matches!(&access.constant, ClassLikeConstantSelector::Identifier(ident)
                    if bytes_to_str(ident.value).eq_ignore_ascii_case("class"))
        );
    if names_class && !matches!(method, Expression::Literal(Literal::String(_))) {
        ctx.use_every(MemberKind::Method);
    }
}

/// Record the property a write in `expr` targets.  `stores` is whether
/// the write replaces the value without reading it (a plain `=`, an
/// `unset`, a `foreach` target); a compound assignment, `++`, or a
/// reference reads it too.  A write through an array offset or a
/// destructuring pattern counts as a write to the property underneath.
fn mark_target(expr: &Expression<'_>, stores: bool, ctx: &mut Scan<'_>) {
    mark_target_reaching(expr, stores.then_some(Reach::Store), ctx);
}

fn mark_target_reaching(expr: &Expression<'_>, store: Option<Reach>, ctx: &mut Scan<'_>) {
    match expr {
        Expression::Access(Access::Property(access)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &access.property {
                ctx.write_property(bytes_to_str(ident.value));
                if let Some(reach) = store {
                    ctx.stores.insert(ident.span.start.offset, reach);
                }
            }
        }
        Expression::Access(Access::StaticProperty(access)) => {
            if let Variable::Direct(variable) = &access.property {
                ctx.write_property(&property_name(variable.name));
                if let Some(reach) = store {
                    ctx.stores.insert(variable.span.start.offset, reach);
                }
            }
        }
        Expression::ArrayAccess(access) => {
            mark_target_reaching(access.array, store.map(|_| Reach::OffsetStore), ctx)
        }
        Expression::ArrayAppend(append) => {
            mark_target_reaching(append.array, store.map(|_| Reach::OffsetStore), ctx)
        }
        Expression::Array(array) => mark_destructured(&array.elements, store, ctx),
        Expression::LegacyArray(array) => mark_destructured(&array.elements, store, ctx),
        Expression::List(list) => mark_destructured(&list.elements, store, ctx),
        Expression::Parenthesized(inner) => mark_target_reaching(inner.expression, store, ctx),
        Expression::UnaryPrefix(unary) if unary.operator.is_reference() => {
            mark_target_reaching(unary.operand, None, ctx)
        }
        _ => {}
    }
}

fn mark_destructured(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    store: Option<Reach>,
    ctx: &mut Scan<'_>,
) {
    for element in elements.iter() {
        let value = match element {
            ArrayElement::KeyValue(entry) => entry.value,
            ArrayElement::Value(entry) => entry.value,
            ArrayElement::Variadic(entry) => entry.value,
            ArrayElement::Missing(_) => continue,
        };
        mark_target_reaching(value, store, ctx);
    }
}

/// What every class body in a parsed file declares private, and what the
/// body does with it.
fn scan_bodies(
    program: &mago_syntax::cst::Program<'_>,
    content: &str,
    names: Option<&OwnedResolvedNames>,
) -> (Vec<Body>, Vec<PassedToFunction>) {
    let mut scan = Scan {
        trivia: program.trivia.as_slice(),
        content,
        names,
        open: Vec::new(),
        done: Vec::new(),
        stores: HashMap::new(),
        passed: Vec::new(),
    };
    let walker = UsageWalker;
    for statement in program.statements.iter() {
        mago_syntax::walker::Walker::walk_statement(&walker, statement, &mut scan);
    }
    (scan.done, scan.passed)
}

// ── Traits ──────────────────────────────────────────────────────────────────

/// What the source of the traits a class uses does with its members.
///
/// A trait runs in the scope of the class that uses it, so its code may
/// reach the class's private members.  Its source is matched by name: a
/// name it spells uses the member, an access by dynamic name may reach
/// any member, and `$this` handed to a call may reach any property.
/// Returns `false` when a trait's source cannot be read, and nothing can
/// be said.
fn apply_trait_sources(
    backend: &Backend,
    file: (&str, &str),
    loader: &crate::class_lookup::ResolvedClassLoader<'_>,
    body: &mut Body,
) -> bool {
    let mut seen: HashSet<String> = HashSet::new();
    let mut pending = std::mem::take(&mut body.traits);
    while let Some(fqn) = pending.pop() {
        if !seen.insert(fqn.to_ascii_lowercase()) {
            continue;
        }
        let Some(r#trait) = loader(Fqn::new(&fqn)) else {
            return false;
        };
        let Some(uri) = backend.class_declaration_uri(&r#trait.fqn()) else {
            return false;
        };
        let other_file;
        let text = if uri == file.0 {
            file.1
        } else {
            let Some(content) = backend.get_file_content_arc(&uri) else {
                return false;
            };
            other_file = content;
            other_file.as_str()
        };
        let Some(source) = text.get(r#trait.start_offset as usize..=r#trait.end_offset as usize)
        else {
            return false;
        };
        apply_trait_source(source, body);
        pending.extend(r#trait.used_traits.iter().map(|t| t.to_string()));
    }
    true
}

fn apply_trait_source(source: &str, body: &mut Body) {
    let dynamic_member = ["->$", "->{", "::{", "::$$", "::${"]
        .iter()
        .any(|pattern| source.contains(pattern));
    if dynamic_member || hands_this_over(source) {
        for member in &mut body.members {
            if member.kind == MemberKind::Property || dynamic_member {
                member.mark_all();
            }
        }
    }
    let lowered = body
        .members
        .iter()
        .any(|m| m.kind == MemberKind::Method && !m.read)
        .then(|| source.to_ascii_lowercase());
    for member in &mut body.members {
        if member.read && member.written {
            continue;
        }
        let spelled = match (member.kind, &lowered) {
            (MemberKind::Method, Some(lowered)) => {
                spells_word(lowered, &member.name.to_ascii_lowercase())
            }
            _ => spells_word(source, &member.name),
        };
        if spelled {
            member.mark_all();
        }
    }
}

/// Whether `source` passes `$this` as an argument.
fn hands_this_over(source: &str) -> bool {
    source.match_indices("$this").any(|(at, word)| {
        let after = source[at + word.len()..].trim_start();
        after.starts_with(',') || after.starts_with(')')
    })
}

/// Whether `word` appears in `source` as a whole identifier.
fn spells_word(source: &str, word: &str) -> bool {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
    let bytes = source.as_bytes();
    source.match_indices(word).any(|(at, _)| {
        let before = at.checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(at + word.len()).copied();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

// ── Collection ──────────────────────────────────────────────────────────────

impl Backend {
    /// The private members the classes in a file declare and do not use.
    pub(crate) fn unused_private_members(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
    ) -> Vec<UnusedMember> {
        let _parse_guard = with_parse_cache(content);
        let names = ctx.file.resolved_names.as_deref();
        let (mut bodies, passed) =
            with_parsed_program(content, "unused_private_members", |program, content| {
                scan_bodies(program, content, names)
            });
        bodies.retain(|body| !body.members.is_empty());
        if bodies.is_empty() {
            return Vec::new();
        }
        if !passed.is_empty() {
            let loaders = self.file_loaders(&ctx.file);
            for argument in &passed {
                if !takes_by_reference(&loaders, argument) {
                    continue;
                }
                for body in bodies
                    .iter_mut()
                    .filter(|body| argument.bodies.contains(&body.left_brace))
                {
                    for member in body.members_of(MemberKind::Property) {
                        if member.name == argument.property {
                            member.written = true;
                        }
                    }
                }
            }
        }

        let class_loaders = self.resolved_class_loaders(&ctx.file);
        let mut found = Vec::new();
        for body in &mut bodies {
            let Some(class) = ctx
                .file
                .classes
                .iter()
                .find(|class| class.start_offset == body.left_brace)
            else {
                continue;
            };
            if !body.traits.is_empty()
                && !apply_trait_sources(
                    self,
                    (uri, content),
                    class_loaders.at(body.left_brace),
                    body,
                )
            {
                continue;
            }
            if self.is_doctrine_project()
                && let Some((_, fields)) = crate::doctrine::mapping_files::mapped_fields(
                    &self.doctrine_mapping_files,
                    &class.fqn(),
                )
            {
                for member in body.members_of(MemberKind::Property) {
                    if fields.contains(&member.name) {
                        member.mark_all();
                    }
                }
            }

            let owner = (!class.name.starts_with("__anonymous@")).then(|| class.fqn());
            for member in &body.members {
                if let Some(unused) = judge(member, owner.as_deref()) {
                    found.push(unused);
                }
            }
        }
        found
    }

    /// Collect the diagnostics for the private members a file's classes
    /// do not use.
    pub(crate) fn collect_unused_private_member_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        for member in self.unused_private_members(ctx, uri, content) {
            if let Some(diagnostic) = self.unused_member_diagnostic(uri, content, &member) {
                out.push(diagnostic);
            }
        }
    }

    pub(crate) fn unused_member_diagnostic(
        &self,
        uri: &str,
        content: &str,
        member: &UnusedMember,
    ) -> Option<Diagnostic> {
        let range = self.offset_range_to_lsp_range(
            uri,
            content,
            member.start as usize,
            member.end as usize,
        )?;
        let (severity, tag) = match member.usage {
            Usage::OnlyRead => (DiagnosticSeverity::Information, None),
            _ => (DiagnosticSeverity::Hint, Some(DiagnosticTag::Unnecessary)),
        };
        Some(make_tagged_diagnostic(
            range,
            severity,
            member.code(),
            member.message(),
            tag,
        ))
    }
}

/// Whether the function `argument` is passed to may write it: it takes
/// the argument by reference, or it cannot be found.
fn takes_by_reference(
    loaders: &crate::resolution::FileLoaders<'_>,
    argument: &PassedToFunction,
) -> bool {
    let Some(function) =
        loaders.function_loader_at(argument.offset)(&argument.function, argument.offset)
    else {
        return true;
    };
    let parameters = &function.parameters;
    let parameter = match &argument.name {
        Some(name) => parameters
            .iter()
            .find(|p| p.name.strip_prefix('$').unwrap_or(&p.name) == name),
        None => parameters
            .get(argument.position)
            .or_else(|| parameters.last().filter(|p| p.is_variadic)),
    };
    parameter.is_none_or(|parameter| parameter.is_reference)
}

/// Whether, and how, `member` goes unused.
fn judge(member: &Declared, owner: Option<&str>) -> Option<UnusedMember> {
    let usage = match member.kind {
        MemberKind::Method | MemberKind::Constant if !member.read => Usage::Unused,
        MemberKind::Method | MemberKind::Constant => return None,
        MemberKind::Property => match (member.read, member.written) {
            (true, true) => return None,
            (false, false) => Usage::Unused,
            // A hooked property may be virtual, its hooks doing the
            // reading and writing.
            _ if member.is_hooked => return None,
            (false, true) => Usage::OnlyWritten,
            // A default is the value the class means it to hold.
            (true, false) if member.starts_initialized || member.checked_for_initialization => {
                return None;
            }
            (true, false) => Usage::OnlyRead,
        },
    };
    let label = match (member.kind, owner) {
        (MemberKind::Property, Some(owner)) => format!("{owner}::${}", member.name),
        (MemberKind::Property, None) => format!("${}", member.name),
        (MemberKind::Method, Some(owner)) => format!("{owner}::{}()", member.name),
        (MemberKind::Method, None) => format!("{}()", member.name),
        (MemberKind::Constant, Some(owner)) => format!("{owner}::{}", member.name),
        (MemberKind::Constant, None) => member.name.clone(),
    };
    // Removing a property its class still writes would leave the writes
    // behind.
    let removal = match usage {
        Usage::Unused => member.removal,
        _ => None,
    };
    Some(UnusedMember {
        kind: member.kind,
        usage,
        label,
        start: member.start,
        end: member.end,
        removal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spells_word_needs_identifier_boundaries() {
        assert!(spells_word("$this->total + 1", "total"));
        assert!(!spells_word("$this->subtotal", "total"));
        assert!(!spells_word("$this->totals", "total"));
    }

    #[test]
    fn hands_this_over_sees_an_argument() {
        assert!(hands_this_over("serialize($this)"));
        assert!(hands_this_over("f($this , 1)"));
        assert!(!hands_this_over("$this->x"));
    }

    #[test]
    fn line_removal_takes_the_whole_line() {
        let content = "class A {\n    private $a;\n    private $b;\n}\n";
        let start = content.find("private $a").unwrap() as u32;
        let end = start + "private $a;".len() as u32;
        let (from, to) = line_removal(content, start, end);
        assert_eq!(
            format!("{}{}", &content[..from as usize], &content[to as usize..]),
            "class A {\n    private $b;\n}\n"
        );
    }

    #[test]
    fn line_removal_drops_a_blank_line_left_before_the_brace() {
        let content = "class A {\n    private $a;\n\n    private $b;\n}\n";
        let start = content.find("private $b").unwrap() as u32;
        let end = start + "private $b;".len() as u32;
        let (from, to) = line_removal(content, start, end);
        assert_eq!(
            format!("{}{}", &content[..from as usize], &content[to as usize..]),
            "class A {\n    private $a;\n}\n"
        );
    }
}
