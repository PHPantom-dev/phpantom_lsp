//! Reads of a fresh object's typed property before anything writes it.
//!
//! ```php
//! final class Options { public string $host; }
//!
//! $options = new Options();
//! echo $options->host; // Error: must not be accessed before initialization
//! ```
//!
//! A public property is routinely filled by whoever creates the object, so
//! the declaration alone says nothing.  The read does: an object created
//! with `new` and stored in a variable is followed through the statements
//! after it, and a read of a property its class leaves uninitialized is
//! reported when nothing wrote that property first.
//!
//! The object stops being followed the moment anything else could reach
//! it: the variable passed anywhere, returned, copied, captured by a
//! closure, reassigned, or given a method call, and anything that can
//! touch every local variable (`extract()`, `include`, a variable
//! variable).  A loop counts every write in its body as having happened
//! before its first iteration, and a jump (`goto`) ends the trail, so a
//! write that may run before the read is never missed.
//!
//! On the class side, a property counts as uninitialized after `new` only
//! when it is typed, has no default, no framework fills it (the same
//! exclusions as the declaration check), and no class up the chain writes
//! it anywhere in its body or may write properties this check cannot
//! follow.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::Program;
use mago_syntax::cst::access::{NullSafePropertyAccess, PropertyAccess};
use mago_syntax::cst::argument::{NamedArgument, PositionalArgument};
use mago_syntax::cst::array::ArrayElement;
use mago_syntax::cst::assignment::{Assignment, AssignmentOperator};
use mago_syntax::cst::binary::{Binary, BinaryOperator};
use mago_syntax::cst::block::Block;
use mago_syntax::cst::call::{FunctionCall, MethodCall, NullSafeMethodCall, StaticMethodCall};
use mago_syntax::cst::class_like::member::ClassLikeMemberSelector;
use mago_syntax::cst::class_like::{AnonymousClass, Class, Enum, Interface, Trait};
use mago_syntax::cst::construct::{
    Construct, EmptyConstruct, EvalConstruct, IncludeConstruct, IncludeOnceConstruct,
    IssetConstruct, RequireConstruct, RequireOnceConstruct,
};
use mago_syntax::cst::expression::Expression;
use mago_syntax::cst::function_like::arrow_function::ArrowFunction;
use mago_syntax::cst::function_like::closure::Closure;
use mago_syntax::cst::function_like::function::Function;
use mago_syntax::cst::r#loop::do_while::DoWhile;
use mago_syntax::cst::r#loop::r#for::For;
use mago_syntax::cst::r#loop::foreach::{Foreach, ForeachKeyValueTarget, ForeachValueTarget};
use mago_syntax::cst::r#loop::r#while::While;
use mago_syntax::cst::namespace::NamespaceImplicitBody;
use mago_syntax::cst::sequence::Sequence;
use mago_syntax::cst::statement::Statement;
use mago_syntax::cst::unary::{UnaryPrefix, UnaryPrefixOperator};
use mago_syntax::cst::unset::Unset;
use mago_syntax::cst::variable::{DirectVariable, IndirectVariable, Variable};
use mago_syntax::cst::{
    ClosureUseClauseVariable, FunctionLikeParameter, Global, Goto, Instantiation, Label, Method,
    PropertyHook, Static,
};
use mago_syntax::walker::Walker;
use tower_lsp_server::gen_lsp_types::{Diagnostic, DiagnosticSeverity};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::class_lookup::Fqn;
use crate::names::OwnedResolvedNames;
use crate::parser::with_parsed_program;
use crate::types::{ClassInfo, ClassLikeKind, MAX_INHERITANCE_DEPTH, PhpVersion};

use super::helpers::{FileDiagnosticContext, make_diagnostic};
use super::uninitialized_properties::{ClassBody, UNINITIALIZED_PROPERTY_CODE, class_bodies};

/// A read of a property of an object created with `new`, before anything
/// wrote the property.
pub(super) struct FreshRead {
    /// The variable holding the object, with its `$`.
    variable: Vec<u8>,
    /// FQN of the instantiated class, without a leading `\`.
    class: String,
    property: String,
    /// Byte range of the property access.
    start: u32,
    end: u32,
    /// Whether the access is a call argument or a reference, which a
    /// by-reference parameter or the reference itself initializes to
    /// `null` when the property's type allows it.
    may_bind_reference: bool,
}

/// Find the reads in `program` of a fresh object's property before
/// anything wrote it.  The class side is not consulted yet.
pub(super) fn fresh_object_reads(
    program: &Program<'_>,
    content: &str,
    names: Option<&OwnedResolvedNames>,
) -> Vec<FreshRead> {
    let Some(names) = names else {
        return Vec::new();
    };
    let mut lists = Lists {
        content,
        names,
        function_depth: 0,
        reads: Vec::new(),
    };
    scan_statements(&program.statements, &mut lists);
    for statement in program.statements.iter() {
        ListFinder.walk_statement(statement, &mut lists);
    }
    if lists.reads.is_empty() {
        return lists.reads;
    }

    // A variable bound by reference anywhere in the file may be written
    // through its alias by code that never names it.
    let mut aliased = HashSet::new();
    for statement in program.statements.iter() {
        AliasFinder.walk_statement(statement, &mut aliased);
    }
    lists
        .reads
        .retain(|read| !aliased.contains(read.variable.as_slice()));
    lists.reads
}

struct Lists<'a> {
    content: &'a str,
    names: &'a OwnedResolvedNames,
    /// Depth inside function-like bodies; zero in the global scope.
    function_depth: u32,
    reads: Vec<FreshRead>,
}

/// Finds every statement list in the file and scans it.
struct ListFinder;

impl<'ast, 'arena> Walker<'ast, 'arena, Lists<'_>> for ListFinder {
    fn walk_in_block(&self, node: &'ast Block<'arena>, ctx: &mut Lists<'_>) {
        scan_statements(&node.statements, ctx);
    }

    fn walk_in_function(&self, _node: &'ast Function<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth += 1;
    }

    fn walk_out_function(&self, _node: &'ast Function<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth -= 1;
    }

    fn walk_in_method(&self, _node: &'ast Method<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth += 1;
    }

    fn walk_out_method(&self, _node: &'ast Method<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth -= 1;
    }

    fn walk_in_property_hook(&self, _node: &'ast PropertyHook<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth += 1;
    }

    fn walk_out_property_hook(&self, _node: &'ast PropertyHook<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth -= 1;
    }

    fn walk_in_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth += 1;
    }

    fn walk_out_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut Lists<'_>) {
        ctx.function_depth -= 1;
    }

    fn walk_in_namespace_implicit_body(
        &self,
        node: &'ast NamespaceImplicitBody<'arena>,
        ctx: &mut Lists<'_>,
    ) {
        scan_statements(&node.statements, ctx);
    }
}

/// Follow every object a statement in `statements` creates through the
/// statements after it.
fn scan_statements(statements: &Sequence<'_, Statement<'_>>, lists: &mut Lists<'_>) {
    let mut state = ScanState {
        content: lists.content,
        global_scope: lists.function_depth == 0,
        objects: Vec::new(),
        nested: 0,
        guarded: 0,
        prescan: false,
        reads: Vec::new(),
    };
    for statement in statements.iter() {
        if !state.objects.is_empty() && !declares_symbol(statement) {
            Scan.walk_statement(statement, &mut state);
        }
        if ends_flow(statement) {
            state.objects.clear();
        } else if let Some(object) = fresh_object(statement, lists.names) {
            state.objects.push(object);
        }
    }
    lists.reads.append(&mut state.reads);
}

/// A declaration hoisted out of the statement list, with a scope of its
/// own.
fn declares_symbol(statement: &Statement<'_>) -> bool {
    matches!(
        statement,
        Statement::Function(_)
            | Statement::Class(_)
            | Statement::Interface(_)
            | Statement::Trait(_)
            | Statement::Enum(_)
    )
}

/// Whether nothing after `statement` in its list runs.
fn ends_flow(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::Return(_) => true,
        Statement::Expression(statement) => matches!(
            statement.expression,
            Expression::Throw(_)
                | Expression::Construct(Construct::Exit(_))
                | Expression::Construct(Construct::Die(_))
        ),
        _ => false,
    }
}

/// The object `$var = new Name(…);` creates.
fn fresh_object(statement: &Statement<'_>, names: &OwnedResolvedNames) -> Option<FreshObject> {
    let Statement::Expression(statement) = statement else {
        return None;
    };
    let Expression::Assignment(assignment) = statement.expression else {
        return None;
    };
    if !matches!(assignment.operator, AssignmentOperator::Assign(_)) {
        return None;
    }
    let Expression::Variable(Variable::Direct(variable)) = assignment.lhs else {
        return None;
    };
    let Expression::Instantiation(instantiation) = assignment.rhs else {
        return None;
    };
    let Expression::Identifier(identifier) = instantiation.class else {
        return None;
    };
    if variable.name == b"$this" {
        return None;
    }
    let fqn = names.get(identifier.span().start.offset)?;
    Some(FreshObject {
        variable: variable.name.to_vec(),
        class: fqn.strip_prefix('\\').unwrap_or(fqn).to_string(),
        settled: HashSet::new(),
    })
}

/// An object followed from the `new` that created it.
#[derive(Clone)]
struct FreshObject {
    /// The variable holding it, with its `$`.
    variable: Vec<u8>,
    class: String,
    /// Properties written, or already read once, since the `new`.
    settled: HashSet<String>,
}

struct ScanState<'a> {
    content: &'a str,
    /// Whether the statements run in the global scope, where any function
    /// may reach a variable through `global` or `$GLOBALS`.
    global_scope: bool,
    objects: Vec<FreshObject>,
    /// Depth inside a closure, a function, or a class body, where the
    /// variable names belong to another scope (or are captured).
    nested: u32,
    /// Depth inside `isset()`, `empty()`, or the left side of `??`, which
    /// read an uninitialized property without throwing.
    guarded: u32,
    /// Whether this is the look-ahead over a loop body, which only
    /// collects writes and escapes.
    prescan: bool,
    reads: Vec<FreshRead>,
}

impl ScanState<'_> {
    /// Stop following the object `variable` holds.
    fn escape(&mut self, variable: &[u8]) {
        if !self.objects.is_empty() {
            self.objects.retain(|object| object.variable != variable);
        }
    }

    fn object_mut(&mut self, variable: &[u8]) -> Option<&mut FreshObject> {
        self.objects
            .iter_mut()
            .find(|object| object.variable == variable)
    }

    fn write(&mut self, variable: &[u8], property: &[u8]) {
        if let Some(object) = self.object_mut(variable) {
            object.settled.insert(bytes_to_str(property).to_string());
        }
    }

    fn read(&mut self, variable: &[u8], property: &[u8], start: u32, end: u32, reference: bool) {
        // In the look-ahead, an access that may bind a reference counts as
        // a write: it initializes a nullable property.
        if self.prescan {
            if reference {
                self.write(variable, property);
            }
            return;
        }
        let Some(object) = self.object_mut(variable) else {
            return;
        };
        let property = bytes_to_str(property);
        if object.settled.contains(property) {
            return;
        }
        object.settled.insert(property.to_string());
        let class = object.class.clone();
        let variable = object.variable.clone();
        self.reads.push(FreshRead {
            variable,
            class,
            property: property.to_string(),
            start,
            end,
            may_bind_reference: reference,
        });
    }

    /// After a call returns: in the global scope, the callee may have
    /// reached any variable through `global` or `$GLOBALS`.
    fn leave_call(&mut self) {
        if self.global_scope {
            self.objects.clear();
        }
    }

    /// Run `walk` as a look-ahead and apply what it found: an object that
    /// escapes anywhere in it stops being followed, and a property written
    /// anywhere in it counts as written from here on.
    fn look_ahead(&mut self, walk: impl FnOnce(&mut ScanState<'_>)) {
        if self.prescan || self.objects.is_empty() {
            return;
        }
        let mut ahead = ScanState {
            content: self.content,
            global_scope: self.global_scope,
            objects: self.objects.clone(),
            nested: self.nested,
            guarded: 0,
            prescan: true,
            reads: Vec::new(),
        };
        walk(&mut ahead);
        self.objects.retain(|object| {
            ahead
                .objects
                .iter()
                .any(|other| other.variable == object.variable)
        });
        for object in &mut self.objects {
            if let Some(other) = ahead
                .objects
                .iter_mut()
                .find(|other| other.variable == object.variable)
            {
                object.settled.extend(other.settled.drain());
            }
        }
    }
}

/// `$var->name`, with `$var` a plain variable and `name` spelled out.
fn plain_property<'x>(
    object: &'x Expression<'x>,
    property: &'x ClassLikeMemberSelector<'x>,
) -> Option<(&'x DirectVariable<'x>, &'x [u8])> {
    match (object, property) {
        (
            Expression::Variable(Variable::Direct(variable)),
            ClassLikeMemberSelector::Identifier(name),
        ) => Some((variable, name.value)),
        _ => None,
    }
}

/// [`plain_property`] for an expression that may be either kind of
/// property access.
fn as_plain_property<'x>(expr: &'x Expression<'x>) -> Option<(&'x DirectVariable<'x>, &'x [u8])> {
    match expr {
        Expression::Access(mago_syntax::cst::access::Access::Property(access)) => {
            plain_property(access.object, &access.property)
        }
        Expression::Access(mago_syntax::cst::access::Access::NullSafeProperty(access)) => {
            plain_property(access.object, &access.property)
        }
        _ => None,
    }
}

/// Follows the fresh objects through a run of statements.
struct Scan;

impl Scan {
    fn access(
        &self,
        variable: &DirectVariable<'_>,
        property: &[u8],
        span: mago_span::Span,
        reference: bool,
        ctx: &mut ScanState<'_>,
    ) {
        if ctx.nested > 0 {
            ctx.escape(variable.name);
        } else if ctx.guarded == 0 {
            ctx.read(
                variable.name,
                property,
                span.start.offset,
                span.end.offset,
                reference,
            );
        }
    }

    /// Walk `expr` as an argument or a reference target, which may bind
    /// a reference to it.
    fn walk_bindable(&self, expr: &Expression<'_>, ctx: &mut ScanState<'_>) {
        match as_plain_property(expr) {
            Some((variable, property)) => self.access(variable, property, expr.span(), true, ctx),
            None => self.walk_expression(expr, ctx),
        }
    }

    /// Walk the target of a write.  A write to `$var->name`, directly or
    /// through an offset or a destructuring pattern, initializes it.
    fn walk_target(&self, expr: &Expression<'_>, ctx: &mut ScanState<'_>) {
        match expr {
            Expression::Access(mago_syntax::cst::access::Access::Property(access))
                if let Some((variable, property)) =
                    plain_property(access.object, &access.property) =>
            {
                if ctx.nested > 0 {
                    ctx.escape(variable.name);
                } else {
                    ctx.write(variable.name, property);
                }
            }
            Expression::ArrayAccess(access) => {
                self.walk_target(access.array, ctx);
                self.walk_expression(access.index, ctx);
            }
            Expression::ArrayAppend(append) => self.walk_target(append.array, ctx),
            Expression::Array(array) => self.walk_destructured(&array.elements, ctx),
            Expression::LegacyArray(array) => self.walk_destructured(&array.elements, ctx),
            Expression::List(list) => self.walk_destructured(&list.elements, ctx),
            Expression::Parenthesized(inner) => self.walk_target(inner.expression, ctx),
            Expression::UnaryPrefix(unary) if unary.operator.is_reference() => {
                self.walk_target(unary.operand, ctx)
            }
            _ => self.walk_expression(expr, ctx),
        }
    }

    fn walk_destructured(
        &self,
        elements: &mago_syntax::cst::sequence::TokenSeparatedSequence<'_, ArrayElement<'_>>,
        ctx: &mut ScanState<'_>,
    ) {
        for element in elements.iter() {
            match element {
                ArrayElement::KeyValue(entry) => {
                    self.walk_expression(entry.key, ctx);
                    self.walk_target(entry.value, ctx);
                }
                ArrayElement::Value(entry) => self.walk_target(entry.value, ctx),
                ArrayElement::Variadic(entry) => self.walk_target(entry.value, ctx),
                ArrayElement::Missing(_) => {}
            }
        }
    }

    fn guarded(&self, expr: &Expression<'_>, ctx: &mut ScanState<'_>) {
        ctx.guarded += 1;
        self.walk_expression(expr, ctx);
        ctx.guarded -= 1;
    }
}

impl<'ast, 'arena> Walker<'ast, 'arena, ScanState<'_>> for Scan {
    fn walk_property_access(&self, node: &'ast PropertyAccess<'arena>, ctx: &mut ScanState<'_>) {
        match plain_property(node.object, &node.property) {
            Some((variable, property)) => self.access(variable, property, node.span(), false, ctx),
            None => mago_syntax::walker::walk_property_access(self, node, ctx),
        }
    }

    fn walk_null_safe_property_access(
        &self,
        node: &'ast NullSafePropertyAccess<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        match plain_property(node.object, &node.property) {
            Some((variable, property)) => self.access(variable, property, node.span(), false, ctx),
            None => mago_syntax::walker::walk_null_safe_property_access(self, node, ctx),
        }
    }

    fn walk_assignment(&self, node: &'ast Assignment<'arena>, ctx: &mut ScanState<'_>) {
        match node.operator {
            // The value is computed before the target is written.
            AssignmentOperator::Assign(_) => {
                self.walk_expression(node.rhs, ctx);
                self.walk_target(node.lhs, ctx);
            }
            // `??=` checks the target the way `isset()` does.
            AssignmentOperator::Coalesce(_) => {
                self.walk_expression(node.rhs, ctx);
                self.walk_target(node.lhs, ctx);
            }
            // A compound assignment reads the target first.
            _ => {
                self.walk_expression(node.lhs, ctx);
                self.walk_expression(node.rhs, ctx);
            }
        }
    }

    fn walk_unary_prefix(&self, node: &'ast UnaryPrefix<'arena>, ctx: &mut ScanState<'_>) {
        if let UnaryPrefixOperator::Reference(_) = node.operator {
            self.walk_bindable(node.operand, ctx);
        } else {
            mago_syntax::walker::walk_unary_prefix(self, node, ctx);
        }
    }

    fn walk_positional_argument(
        &self,
        node: &'ast PositionalArgument<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        self.walk_bindable(node.value, ctx);
    }

    fn walk_named_argument(&self, node: &'ast NamedArgument<'arena>, ctx: &mut ScanState<'_>) {
        self.walk_bindable(node.value, ctx);
    }

    fn walk_isset_construct(&self, node: &'ast IssetConstruct<'arena>, ctx: &mut ScanState<'_>) {
        for value in node.values.iter() {
            self.guarded(value, ctx);
        }
    }

    fn walk_empty_construct(&self, node: &'ast EmptyConstruct<'arena>, ctx: &mut ScanState<'_>) {
        self.guarded(node.value, ctx);
    }

    fn walk_unset(&self, node: &'ast Unset<'arena>, ctx: &mut ScanState<'_>) {
        // `unset()` leaves the property in a state this check does not
        // follow, so it counts as a write.
        for value in node.values.iter() {
            ctx.guarded += 1;
            self.walk_target(value, ctx);
            ctx.guarded -= 1;
        }
    }

    fn walk_binary(&self, node: &'ast Binary<'arena>, ctx: &mut ScanState<'_>) {
        if let BinaryOperator::NullCoalesce(_) = node.operator {
            self.guarded(node.lhs, ctx);
            self.walk_expression(node.rhs, ctx);
        } else {
            mago_syntax::walker::walk_binary(self, node, ctx);
        }
    }

    fn walk_foreach_value_target(
        &self,
        node: &'ast ForeachValueTarget<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        self.walk_target(node.value, ctx);
    }

    fn walk_foreach_key_value_target(
        &self,
        node: &'ast ForeachKeyValueTarget<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        self.walk_target(node.key, ctx);
        self.walk_target(node.value, ctx);
    }

    // ── Loops: every write in the body may run before any read in it ────

    fn walk_in_foreach(&self, node: &'ast Foreach<'arena>, ctx: &mut ScanState<'_>) {
        ctx.look_ahead(|ahead| mago_syntax::walker::walk_foreach(self, node, ahead));
    }

    fn walk_in_for(&self, node: &'ast For<'arena>, ctx: &mut ScanState<'_>) {
        ctx.look_ahead(|ahead| mago_syntax::walker::walk_for(self, node, ahead));
    }

    fn walk_in_while(&self, node: &'ast While<'arena>, ctx: &mut ScanState<'_>) {
        ctx.look_ahead(|ahead| mago_syntax::walker::walk_while(self, node, ahead));
    }

    fn walk_in_do_while(&self, node: &'ast DoWhile<'arena>, ctx: &mut ScanState<'_>) {
        ctx.look_ahead(|ahead| mago_syntax::walker::walk_do_while(self, node, ahead));
    }

    // ── Escapes ─────────────────────────────────────────────────────────

    fn walk_in_direct_variable(&self, node: &'ast DirectVariable<'arena>, ctx: &mut ScanState<'_>) {
        ctx.escape(node.name);
    }

    fn walk_in_indirect_variable(
        &self,
        _node: &'ast IndirectVariable<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.objects.clear();
    }

    fn walk_out_function_call(&self, _node: &'ast FunctionCall<'arena>, ctx: &mut ScanState<'_>) {
        ctx.leave_call();
    }

    fn walk_out_method_call(&self, _node: &'ast MethodCall<'arena>, ctx: &mut ScanState<'_>) {
        ctx.leave_call();
    }

    fn walk_out_null_safe_method_call(
        &self,
        _node: &'ast NullSafeMethodCall<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.leave_call();
    }

    fn walk_out_static_method_call(
        &self,
        _node: &'ast StaticMethodCall<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.leave_call();
    }

    fn walk_out_instantiation(&self, _node: &'ast Instantiation<'arena>, ctx: &mut ScanState<'_>) {
        ctx.leave_call();
    }

    fn walk_in_function_call(&self, node: &'ast FunctionCall<'arena>, ctx: &mut ScanState<'_>) {
        let Expression::Identifier(identifier) = node.function else {
            return;
        };
        let span = identifier.span();
        let name = ctx
            .content
            .get(span.start.offset as usize..span.end.offset as usize)
            .unwrap_or("");
        let name = name.rsplit('\\').next().unwrap_or(name);
        if ["extract", "compact", "get_defined_vars", "parse_str"]
            .iter()
            .any(|scope_reader| name.eq_ignore_ascii_case(scope_reader))
        {
            ctx.objects.clear();
        }
    }

    fn walk_in_eval_construct(&self, _node: &'ast EvalConstruct<'arena>, ctx: &mut ScanState<'_>) {
        ctx.objects.clear();
    }

    fn walk_in_include_construct(
        &self,
        _node: &'ast IncludeConstruct<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.objects.clear();
    }

    fn walk_in_include_once_construct(
        &self,
        _node: &'ast IncludeOnceConstruct<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.objects.clear();
    }

    fn walk_in_require_construct(
        &self,
        _node: &'ast RequireConstruct<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.objects.clear();
    }

    fn walk_in_require_once_construct(
        &self,
        _node: &'ast RequireOnceConstruct<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.objects.clear();
    }

    fn walk_in_goto(&self, _node: &'ast Goto<'arena>, ctx: &mut ScanState<'_>) {
        ctx.objects.clear();
    }

    fn walk_in_label(&self, _node: &'ast Label<'arena>, ctx: &mut ScanState<'_>) {
        ctx.objects.clear();
    }

    // ── Nested scopes ───────────────────────────────────────────────────

    fn walk_in_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested += 1;
    }

    fn walk_out_closure(&self, _node: &'ast Closure<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested -= 1;
    }

    fn walk_in_arrow_function(&self, _node: &'ast ArrowFunction<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested += 1;
    }

    fn walk_out_arrow_function(&self, _node: &'ast ArrowFunction<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested -= 1;
    }

    fn walk_in_function(&self, _node: &'ast Function<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested += 1;
    }

    fn walk_out_function(&self, _node: &'ast Function<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested -= 1;
    }

    fn walk_in_anonymous_class(
        &self,
        _node: &'ast AnonymousClass<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.nested += 1;
    }

    fn walk_out_anonymous_class(
        &self,
        _node: &'ast AnonymousClass<'arena>,
        ctx: &mut ScanState<'_>,
    ) {
        ctx.nested -= 1;
    }

    fn walk_in_class(&self, _node: &'ast Class<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested += 1;
    }

    fn walk_out_class(&self, _node: &'ast Class<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested -= 1;
    }

    fn walk_in_interface(&self, _node: &'ast Interface<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested += 1;
    }

    fn walk_out_interface(&self, _node: &'ast Interface<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested -= 1;
    }

    fn walk_in_trait(&self, _node: &'ast Trait<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested += 1;
    }

    fn walk_out_trait(&self, _node: &'ast Trait<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested -= 1;
    }

    fn walk_in_enum(&self, _node: &'ast Enum<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested += 1;
    }

    fn walk_out_enum(&self, _node: &'ast Enum<'arena>, ctx: &mut ScanState<'_>) {
        ctx.nested -= 1;
    }
}

/// Collects the variables a file binds by reference: `&$var`, a
/// by-reference parameter or closure capture, `global`, and `static`.
struct AliasFinder;

impl<'ast, 'arena> Walker<'ast, 'arena, HashSet<Vec<u8>>> for AliasFinder {
    fn walk_in_unary_prefix(&self, node: &'ast UnaryPrefix<'arena>, ctx: &mut HashSet<Vec<u8>>) {
        if node.operator.is_reference()
            && let Expression::Variable(Variable::Direct(variable)) = node.operand
        {
            ctx.insert(variable.name.to_vec());
        }
    }

    fn walk_in_function_like_parameter(
        &self,
        node: &'ast FunctionLikeParameter<'arena>,
        ctx: &mut HashSet<Vec<u8>>,
    ) {
        if node.ampersand.is_some() {
            ctx.insert(node.variable.name.to_vec());
        }
    }

    fn walk_in_closure_use_clause_variable(
        &self,
        node: &'ast ClosureUseClauseVariable<'arena>,
        ctx: &mut HashSet<Vec<u8>>,
    ) {
        if node.ampersand.is_some() {
            ctx.insert(node.variable.name.to_vec());
        }
    }

    fn walk_in_global(&self, node: &'ast Global<'arena>, ctx: &mut HashSet<Vec<u8>>) {
        for variable in node.variables.iter() {
            if let Variable::Direct(variable) = variable {
                ctx.insert(variable.name.to_vec());
            }
        }
    }

    fn walk_in_static(&self, node: &'ast Static<'arena>, ctx: &mut HashSet<Vec<u8>>) {
        for item in node.items.iter() {
            ctx.insert(item.variable().name.to_vec());
        }
    }
}

// ── Class side ──────────────────────────────────────────────────────────────

/// For each public property that `new Class` leaves uninitialized, whether
/// its type accepts `null`.
type UninitializedAfterNew = HashMap<String, bool>;

impl Backend {
    pub(super) fn report_uninitialized_reads(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        bodies: &[ClassBody],
        reads: Vec<FreshRead>,
        out: &mut Vec<Diagnostic>,
    ) {
        let mut classes: HashMap<String, Option<UninitializedAfterNew>> = HashMap::new();
        for read in reads {
            if !super::incremental::reports_site(read.start) {
                continue;
            }
            let uninitialized = classes
                .entry(read.class.clone())
                .or_insert_with(|| self.uninitialized_after_new(ctx, uri, bodies, &read.class));
            let Some(accepts_null) = uninitialized
                .as_ref()
                .and_then(|properties| properties.get(&read.property))
            else {
                continue;
            };
            if read.may_bind_reference && *accepts_null {
                continue;
            }
            let Some(range) = self.offset_range_to_lsp_range(
                uri,
                content,
                read.start as usize,
                read.end as usize,
            ) else {
                continue;
            };
            out.push(make_diagnostic(
                range,
                DiagnosticSeverity::Warning,
                UNINITIALIZED_PROPERTY_CODE,
                format!(
                    "Property {}::${} is read before it is initialized",
                    read.class, read.property
                ),
            ));
        }
    }

    /// The public properties `new Class` leaves uninitialized, or `None`
    /// when the class cannot be followed far enough to tell.
    fn uninitialized_after_new(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        bodies: &[ClassBody],
        fqn: &str,
    ) -> Option<UninitializedAfterNew> {
        let class_loader = self.resolved_class_loader(&ctx.file);
        let class = class_loader(Fqn::new(fqn))?;
        if class.kind != ClassLikeKind::Class || class.is_abstract {
            return None;
        }

        let mut chain: Vec<(Arc<ClassInfo>, ClassBody)> = Vec::new();
        let mut next = Some(class);
        while let Some(class) = next.take() {
            if chain.len() as u32 > MAX_INHERITANCE_DEPTH
                || !class.used_traits.is_empty()
                || class.get_method("__get").is_some_and(|m| !m.is_virtual)
                || self.is_mapped_outside_the_class(&class)
            {
                return None;
            }
            let body = self.class_body_of(ctx, uri, bodies, &class)?;
            if body.is_opaque() {
                return None;
            }
            if let Some(parent) = &class.parent_class {
                next = Some(class_loader(Fqn::new(parent))?);
            }
            chain.push((class, body));
        }

        let mut uninitialized = UninitializedAfterNew::new();
        let mut declared_below: HashSet<&str> = HashSet::new();
        for (class, body) in &chain {
            for property in &class.properties {
                let name = property.name.as_str();
                if property.is_virtual
                    || declared_below.contains(name)
                    || !body.starts_uninitialized_in_public(name)
                    || chain.iter().any(|(_, body)| body.writes(name))
                {
                    continue;
                }
                let accepts_null = property
                    .native_type_hint
                    .as_ref()
                    .is_none_or(|hint| hint.accepts_null());
                uninitialized.insert(name.to_string(), accepts_null);
            }
            declared_below.extend(
                class
                    .properties
                    .iter()
                    .map(|property| property.name.as_str())
                    .filter(|name| body.declares(name)),
            );
        }
        Some(uninitialized)
    }

    /// What the body of `class` declares and writes, read from its file.
    fn class_body_of(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        bodies: &[ClassBody],
        class: &ClassInfo,
    ) -> Option<ClassBody> {
        let in_this_file =
            ctx.file.classes.iter().any(|local| {
                local.start_offset == class.start_offset && local.fqn() == class.fqn()
            });
        if in_this_file {
            return bodies
                .iter()
                .position(|body| body.start_offset() == class.start_offset)
                .map(|index| bodies[index].clone());
        }
        let file_uri = self.find_class_file_uri(&class.fqn(), uri)?;
        let content = self.get_file_content_arc(&file_uri)?;
        let names = self
            .file_state(&file_uri)
            .and_then(|state| state.resolved_names.clone());
        let readonly_is_protected_set = self.php_version() >= PhpVersion::new(8, 4);
        with_parsed_program(&content, "uninitialized_properties", |program, content| {
            class_bodies(
                program,
                content,
                names.as_deref(),
                readonly_is_protected_set,
            )
            .into_iter()
            .find(|body| body.start_offset() == class.start_offset)
        })
    }
}
