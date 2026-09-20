//! Readonly property write diagnostics.
//!
//! A `readonly` property may be initialized exactly once, and only from
//! inside the class that declares it.  Every other write is a fatal
//! `Error` at runtime:
//!
//! ```php
//! final class Box {
//!     public function __construct(public readonly int $value) {}
//! }
//!
//! $box = new Box(1);
//! $box->value = 2; // Cannot modify readonly property Box::$value
//! ```
//!
//! Two shapes are reported:
//!
//! - **Outside the declaring class** — the write happens in a scope
//!   that is not the declaring class (top-level code, another class, a
//!   subclass initializing its parent's property).  This is an error
//!   regardless of whether the property was ever initialized.
//! - **After the constructor initialized it** — the write is inside the
//!   declaring class, but the constructor is known to have initialized
//!   the property already (it is a promoted parameter, or the
//!   constructor body assigns it unconditionally), so the write is a
//!   second initialization.
//!
//! Writes inside `__construct`, `__clone` (PHP 8.3 allows readonly
//! reinitialization while cloning), and `__unserialize` are never
//! flagged, and neither is a write whose target the constructor may
//! leave uninitialized: initializing a readonly property lazily from
//! another method of the declaring class is legal PHP.
//!
//! An *indirect* write — an array offset (`$box->items[] = 1`) or a
//! reference (`&$box->value`) — is the exception: PHP rejects those in
//! every scope, including a constructor that has not initialized the
//! property yet, so they are reported wherever they appear.
//!
//! A property carries no `readonly` keyword of its own when its class is
//! declared `readonly`; [`ClassInfo::is_readonly`] is what makes those
//! properties count here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::access::Access;
use mago_syntax::cst::array::ArrayElement;
use mago_syntax::cst::class_like::member::{ClassLikeMember, ClassLikeMemberSelector};
use mago_syntax::cst::class_like::method::MethodBody;
use mago_syntax::cst::class_like::{AnonymousClass, Class};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::r#loop::foreach::{ForeachKeyValueTarget, ForeachValueTarget};
use mago_syntax::cst::sequence::{Sequence, TokenSeparatedSequence};
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::unary::UnaryPrefixOperator;
use mago_syntax::cst::unset::Unset;
use mago_syntax::cst::variable::Variable;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::atom::{Atom, bytes_to_str};
use crate::hover::{MemberKindForOrigin, find_declaring_class};
use crate::parser::{with_parse_cache, with_parsed_program};
use crate::php_type::{PhpType, TypeKind, is_array_like_name};
use crate::symbol_map::SymbolKind;
use crate::type_engine::resolver::{ResolutionCtx, SubjectOutcome, resolve_subject_outcome};
use crate::types::{AccessKind, ClassInfo, ClassLikeKind, PropertyInfo};
use crate::virtual_members::resolve_class_fully_cached;

use super::helpers::{FileDiagnosticContext, find_innermost_enclosing_class, make_diagnostic};

/// Diagnostic code used for writes to a `readonly` property.
pub(crate) const INVALID_READONLY_WRITE_CODE: &str = "invalid_readonly_write";

// ── Collected write sites ───────────────────────────────────────────────────

/// How the source spells a write, which decides both the wording of the
/// diagnostic and whether an uninitialized property saves it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum WriteForm {
    /// An assignment, a compound assignment, an increment, a
    /// destructuring element, or a `foreach` target: legal in the
    /// declaring class while the property may still be uninitialized.
    Modify,
    /// `unset($obj->prop)`, which PHP allows only before the property is
    /// initialized.
    Unset,
    /// A write to an offset of the property's own value
    /// (`$obj->prop[…] = …`, `unset($obj->prop[…])`).  Rejected in every
    /// scope, but only when the property really holds an array: an
    /// `ArrayAccess` object handles the offset itself and leaves the
    /// property alone.
    Offset,
    /// `&$obj->prop`.  Acquiring a reference is rejected in every scope,
    /// whatever the property holds.
    Reference,
}

impl WriteForm {
    /// Whether PHP rejects this form even where the property may still be
    /// uninitialized, i.e. anywhere in the declaring class.
    fn is_indirect(self) -> bool {
        matches!(self, WriteForm::Offset | WriteForm::Reference)
    }

    /// The verb the diagnostic message uses for this form.  Only the two
    /// direct forms reach it; the indirect ones get their own message.
    fn verb(self) -> &'static str {
        match self {
            WriteForm::Unset => "unset",
            _ => "modify",
        }
    }
}

/// Everything the file's AST contributes to the check.
#[derive(Default)]
struct FileWrites {
    /// Byte offsets of property identifiers that appear as the target of
    /// a write, and how that write is spelled.  The offset is the same
    /// one the symbol map records as the start of the matching
    /// `MemberAccess` span.
    targets: HashMap<u32, WriteForm>,
    /// Per class body, keyed by the left-brace offset so it matches
    /// [`ClassInfo::start_offset`].
    classes: HashMap<u32, ClassInitFacts>,
}

/// What a class body says about its own readonly initialization.
#[derive(Default)]
struct ClassInitFacts {
    /// Body ranges of `__construct`, `__clone`, and `__unserialize`,
    /// where a readonly property may legitimately be initialized.
    initializer_ranges: Vec<(u32, u32)>,
    /// Properties the constructor is guaranteed to have initialized by
    /// the time any other method runs: promoted parameters and
    /// unconditional `$this->name = …` statements in its body.
    initialized: HashSet<String>,
}

impl ClassInitFacts {
    fn covers_initializer(&self, offset: u32) -> bool {
        self.initializer_ranges
            .iter()
            .any(|(start, end)| offset >= *start && offset <= *end)
    }
}

// ── AST walk ────────────────────────────────────────────────────────────────

struct WriteWalker;

impl<'ast, 'arena> mago_syntax::walker::Walker<'ast, 'arena, FileWrites> for WriteWalker {
    fn walk_in_expression(&self, node: &'ast Expression<'arena>, ctx: &mut FileWrites) {
        match node {
            Expression::Assignment(assign) => record_target(assign.lhs, WriteForm::Modify, ctx),
            Expression::UnaryPrefix(unary) => match unary.operator {
                UnaryPrefixOperator::PreIncrement(_) | UnaryPrefixOperator::PreDecrement(_) => {
                    record_target(unary.operand, WriteForm::Modify, ctx)
                }
                // `&$obj->prop` in any position — the right-hand side of
                // an assignment, an array element, a `foreach` target.
                UnaryPrefixOperator::Reference(_) => {
                    record_target(unary.operand, WriteForm::Reference, ctx)
                }
                _ => {}
            },
            // The only postfix operators are `++` and `--`.
            Expression::UnaryPostfix(unary) => record_target(unary.operand, WriteForm::Modify, ctx),
            _ => {}
        }
    }

    fn walk_in_unset(&self, node: &'ast Unset<'arena>, ctx: &mut FileWrites) {
        for value in node.values.iter() {
            record_target(value, WriteForm::Unset, ctx);
        }
    }

    fn walk_in_foreach_value_target(
        &self,
        node: &'ast ForeachValueTarget<'arena>,
        ctx: &mut FileWrites,
    ) {
        record_target(node.value, WriteForm::Modify, ctx);
    }

    fn walk_in_foreach_key_value_target(
        &self,
        node: &'ast ForeachKeyValueTarget<'arena>,
        ctx: &mut FileWrites,
    ) {
        record_target(node.key, WriteForm::Modify, ctx);
        record_target(node.value, WriteForm::Modify, ctx);
    }

    fn walk_in_class(&self, node: &'ast Class<'arena>, ctx: &mut FileWrites) {
        ctx.classes.insert(
            node.left_brace.start.offset,
            class_init_facts(&node.members),
        );
    }

    fn walk_in_anonymous_class(&self, node: &'ast AnonymousClass<'arena>, ctx: &mut FileWrites) {
        ctx.classes.insert(
            node.left_brace.start.offset,
            class_init_facts(&node.members),
        );
    }
}

/// Record the property a write in `expr` targets, if any.
///
/// Dynamic selectors (`$obj->$name`) and null-safe access (never a valid
/// write target) are skipped, as are static properties: PHP rejects
/// `static readonly` outright.
///
/// The walk descends through the shapes that put a property somewhere
/// other than the top of the expression: array offsets
/// (`$obj->items[0][1] = …`, which reach the property indirectly),
/// destructuring patterns (`[$obj->a, [$obj->b]] = …`), and references.
fn record_target(expr: &Expression<'_>, form: WriteForm, ctx: &mut FileWrites) {
    match expr {
        Expression::Access(Access::Property(access)) => {
            if let ClassLikeMemberSelector::Identifier(ident) = &access.property {
                let offset = ident.span.start.offset;
                ctx.targets
                    .entry(offset)
                    .and_modify(|existing| *existing = (*existing).max(form))
                    .or_insert(form);
            }
        }
        Expression::ArrayAccess(access) => {
            record_target(access.array, WriteForm::Offset, ctx);
        }
        Expression::ArrayAppend(append) => {
            record_target(append.array, WriteForm::Offset, ctx);
        }
        Expression::Array(array) => record_destructured(&array.elements, ctx),
        Expression::LegacyArray(array) => record_destructured(&array.elements, ctx),
        Expression::List(list) => record_destructured(&list.elements, ctx),
        Expression::Parenthesized(inner) => record_target(inner.expression, form, ctx),
        Expression::UnaryPrefix(unary) if unary.operator.is_reference() => {
            record_target(unary.operand, WriteForm::Reference, ctx)
        }
        _ => {}
    }
}

/// Record the write targets of a destructuring pattern's elements.
///
/// Only the values are targets: a key (`['k' => $obj->prop]`) is read.
fn record_destructured(
    elements: &TokenSeparatedSequence<'_, ArrayElement<'_>>,
    ctx: &mut FileWrites,
) {
    for element in elements.iter() {
        let value = match element {
            ArrayElement::KeyValue(entry) => entry.value,
            ArrayElement::Value(entry) => entry.value,
            ArrayElement::Variadic(entry) => entry.value,
            ArrayElement::Missing(_) => continue,
        };
        record_target(value, WriteForm::Modify, ctx);
    }
}

fn class_init_facts(members: &Sequence<'_, ClassLikeMember<'_>>) -> ClassInitFacts {
    let mut facts = ClassInitFacts::default();

    for member in members.iter() {
        let ClassLikeMember::Method(method) = member else {
            continue;
        };
        let name = bytes_to_str(method.name.value);
        let is_constructor = name.eq_ignore_ascii_case("__construct");
        if !is_constructor
            && !name.eq_ignore_ascii_case("__clone")
            && !name.eq_ignore_ascii_case("__unserialize")
        {
            continue;
        }
        let MethodBody::Concrete(block) = &method.body else {
            continue;
        };
        let span = block.span();
        facts
            .initializer_ranges
            .push((span.start.offset, span.end.offset));

        if !is_constructor {
            continue;
        }

        for param in method.parameter_list.parameters.iter() {
            if param.is_promoted_property() {
                let raw = bytes_to_str(param.variable.name);
                facts
                    .initialized
                    .insert(raw.strip_prefix('$').unwrap_or(raw).to_string());
            }
        }

        // Only statements at the top level of the constructor body count:
        // an assignment inside a branch or a loop may not run, which
        // leaves the property free to be initialized elsewhere.
        for statement in block.statements.iter() {
            if let Statement::Expression(stmt) = statement
                && let Expression::Assignment(assign) = stmt.expression
                && assign.operator.is_assign()
                && let Expression::Access(Access::Property(access)) = assign.lhs
                && is_this(access.object)
                && let ClassLikeMemberSelector::Identifier(ident) = &access.property
            {
                facts
                    .initialized
                    .insert(bytes_to_str(ident.value).to_string());
            }
        }
    }

    facts
}

fn is_this(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Variable(Variable::Direct(var)) if var.name == b"$this")
}

// ── Verdicts ────────────────────────────────────────────────────────────────

enum Verdict {
    /// Nothing to report for this receiver type.
    Ok,
    /// The write is not in the property's declaring scope.
    OutsideDeclaringClass(String),
    /// The write is in the declaring scope, but the constructor has
    /// already initialized the property.
    AlreadyInitialized(String),
    /// The write goes through the property's value (an array offset or a
    /// reference), which no scope may do.
    Indirect(String),
}

/// Whether `class` declares `property` itself, following its traits.
///
/// Trait properties are flattened into the using class, so a class that
/// uses a trait declaring a readonly property *is* the declaring scope
/// for that property as far as PHP is concerned.
fn declares_property(
    class: &ClassInfo,
    property: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    seen: &mut Vec<Atom>,
) -> bool {
    if class
        .properties
        .iter()
        .any(|p| p.name == property && !p.is_virtual)
    {
        return true;
    }

    for trait_name in &class.used_traits {
        if seen.contains(trait_name) {
            continue;
        }
        seen.push(*trait_name);
        if let Some(used) = class_loader(trait_name)
            && declares_property(&used, property, class_loader, seen)
        {
            return true;
        }
    }

    false
}

/// One write the file makes, as the verdict needs to see it.
struct WriteSite<'a> {
    /// Name of the property being written, without the `$`.
    property: &'a str,
    /// How the write is spelled in source.
    form: WriteForm,
    /// Byte offset of the property identifier, used to place the write
    /// relative to the enclosing class's initializer bodies.
    offset: u32,
    /// The class body the write sits in, if any.
    current_class: Option<&'a ClassInfo>,
}

/// Whether an offset write on this property necessarily modifies the
/// property's own value.
///
/// True only when every branch of the declared type is an array: an
/// object routes the offset through `ArrayAccess::offsetSet()` instead,
/// and `iterable` covers objects like `ArrayObject` that do exactly that.
/// An untyped property (a `readonly` one always has a type, so this means
/// the type was lost somewhere) is left alone.
fn holds_an_array(prop: &PropertyInfo) -> bool {
    fn is_array(ty: &PhpType) -> bool {
        match ty.kind() {
            TypeKind::Named(name) => {
                !name.eq_ignore_ascii_case("iterable") && is_array_like_name(name)
            }
            TypeKind::Generic(generic) => {
                !generic.name.eq_ignore_ascii_case("iterable") && is_array_like_name(&generic.name)
            }
            TypeKind::Array(_) | TypeKind::ArrayShape(_) => true,
            TypeKind::Nullable(inner) => is_array(inner),
            TypeKind::Union(members) => members.iter().all(is_array),
            _ => false,
        }
    }

    prop.type_hint.as_ref().is_some_and(is_array)
}

/// How the property is spelled in the diagnostic message.
fn owner_display(declaring: &ClassInfo, property: &str) -> String {
    if declaring.name.starts_with("__anonymous@") {
        format!("${}", property)
    } else {
        format!("{}::${}", declaring.fqn(), property)
    }
}

// ── Main diagnostic collection ──────────────────────────────────────────────

impl Backend {
    /// Collect readonly-write diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub(crate) fn collect_readonly_write_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let _parse_guard = with_parse_cache(content);

        let writes = with_parsed_program(content, "readonly_writes", |program, _content| {
            let mut writes = FileWrites::default();
            let walker = WriteWalker;
            for statement in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, statement, &mut writes);
            }
            writes
        });

        if writes.targets.is_empty() {
            return;
        }

        let local_classes = &ctx.file.classes;
        let class_loader =
            self.class_loader_with(local_classes, &ctx.file.use_map, &ctx.file.namespace);
        let function_loader = self.function_loader_with(
            ctx.file.resolved_names.as_deref(),
            &ctx.file.use_map,
            &ctx.file.namespace,
        );
        let resolved_cache = &self.resolved_class_cache;
        let symbol_map = &ctx.symbol_map;
        let Some(source) = symbol_map.source(content) else {
            return;
        };

        for span in &symbol_map.spans {
            let SymbolKind::MemberAccess {
                subject_text,
                member_name,
                is_static,
                is_method_call,
                ..
            } = &span.kind
            else {
                continue;
            };
            let Some(form) = writes.targets.get(&span.start).copied() else {
                continue;
            };
            if *is_static || *is_method_call {
                continue;
            }

            let current_class = find_innermost_enclosing_class(local_classes, span.start);

            // A trait body is incomplete by nature: the property it
            // writes may be declared by any host class, and the trait
            // itself is flattened into that host's scope.
            if current_class.is_some_and(|class| class.kind == ClassLikeKind::Trait) {
                continue;
            }

            let subject_text = subject_text.as_str(source);

            // `$this->ownProperty = …` inside the constructor is the
            // shape most writes in a file have.  Settling it before
            // resolving the receiver keeps the common case off the type
            // engine.  A property the enclosing class does not declare
            // itself still goes through the full check: initializing a
            // parent's readonly property is an error even in a
            // constructor.  So does an indirect write, which the
            // constructor is no more allowed to make than anyone else.
            if subject_text == "$this"
                && !form.is_indirect()
                && current_class.is_some_and(|class| {
                    writes
                        .classes
                        .get(&class.start_offset)
                        .is_some_and(|facts| facts.covers_initializer(span.start))
                        && declares_property(class, member_name, &class_loader, &mut Vec::new())
                })
            {
                continue;
            }

            let rctx = ResolutionCtx {
                current_class,
                all_classes: local_classes,
                content,
                cursor_offset: span.start,
                class_loader: &class_loader,
                backend: Some(self),
                laravel_macro_this_resolver: None,
                resolved_class_cache: Some(resolved_cache),
                function_loader: Some(&function_loader),
                scope_var_resolver: None,
                is_in_static_method: symbol_map.is_in_static_method(span.start),
                preserve_static: false,
            };
            let SubjectOutcome::Resolved(receivers) =
                resolve_subject_outcome(subject_text, AccessKind::Arrow, &rctx)
            else {
                continue;
            };

            // Every branch of a union has to be invalid before the write
            // is: a value that could hold the branch where the property
            // is writable makes the assignment legal.
            let site = WriteSite {
                property: member_name,
                form,
                offset: span.start,
                current_class,
            };
            let mut verdict: Option<Verdict> = None;
            for receiver in &receivers {
                let judged = self.judge_readonly_write(receiver, &site, &writes, &class_loader);
                if matches!(judged, Verdict::Ok) {
                    verdict = None;
                    break;
                }
                verdict.get_or_insert(judged);
            }

            let message = match verdict {
                Some(Verdict::OutsideDeclaringClass(owner)) => format!(
                    "Cannot {} readonly property {} from outside its declaring class",
                    form.verb(),
                    owner
                ),
                Some(Verdict::AlreadyInitialized(owner)) => format!(
                    "Cannot {} readonly property {} after the constructor has initialized it",
                    form.verb(),
                    owner
                ),
                Some(Verdict::Indirect(owner)) => {
                    format!("Cannot indirectly modify readonly property {}", owner)
                }
                _ => continue,
            };

            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                span.start as usize,
                span.end as usize,
            ) else {
                continue;
            };

            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                INVALID_READONLY_WRITE_CODE,
                message,
            ));
        }
    }

    /// Judge a single write against one of the receiver's possible types.
    fn judge_readonly_write(
        &self,
        receiver: &Arc<ClassInfo>,
        site: &WriteSite<'_>,
        writes: &FileWrites,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Verdict {
        let property = site.property;

        // Object shapes carry all their members already and must not go
        // through the cache: every shape shares the same class name.
        let merged = if receiver.name == "__object_shape" {
            Arc::clone(receiver)
        } else {
            resolve_class_fully_cached(receiver, class_loader, &self.resolved_class_cache)
        };

        let Some(prop) = merged
            .properties
            .iter()
            .find(|p| p.name == property && !p.is_virtual)
        else {
            return Verdict::Ok;
        };
        // A property of a `readonly` class carries no keyword of its own,
        // so the class flag stands in for it.  The flag is inherited, so
        // this also holds for a property reached through the parent chain
        // — but the class that declared it is the one that has to be
        // readonly, which the check below the declaring-class lookup
        // settles.  This is the cheap half: unless something in the
        // hierarchy is readonly, there is nothing to look up.
        if !prop.is_readonly && !merged.is_readonly {
            return Verdict::Ok;
        }

        // The declaring class has to be traced through the *unmerged*
        // class: a merged one lists inherited properties as its own.
        let raw = class_loader(merged.fqn().as_str()).unwrap_or_else(|| Arc::clone(receiver));
        let declaring =
            find_declaring_class(&raw, property, &MemberKindForOrigin::Property, class_loader);

        if !prop.is_readonly && !(declaring.is_readonly && !prop.is_static) {
            return Verdict::Ok;
        }

        let owner = owner_display(&declaring, property);

        if site.form.is_indirect() {
            // Writing through the property's value never counts as
            // initializing it, so PHP rejects it in every scope — but an
            // offset write only reaches the property when the property is
            // the array being written.  On an `ArrayAccess` object it is
            // an `offsetSet()` call, which the property survives.
            if site.form == WriteForm::Offset && !holds_an_array(prop) {
                return Verdict::Ok;
            }
            return Verdict::Indirect(owner);
        }

        let Some(current) = site.current_class else {
            return Verdict::OutsideDeclaringClass(owner);
        };

        let in_declaring_scope = current.fqn() == declaring.fqn()
            || (declaring.kind == ClassLikeKind::Trait
                && declares_property(current, property, class_loader, &mut Vec::new()));
        if !in_declaring_scope {
            return Verdict::OutsideDeclaringClass(owner);
        }

        let Some(facts) = writes.classes.get(&current.start_offset) else {
            return Verdict::Ok;
        };
        if facts.covers_initializer(site.offset) || !facts.initialized.contains(property) {
            return Verdict::Ok;
        }

        Verdict::AlreadyInitialized(owner)
    }
}
