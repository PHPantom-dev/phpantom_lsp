//! The exceptions a `try` body can throw, which tells a `catch` clause no
//! exception can reach from one that may run.
//!
//! A call whose callee declares `@throws` can throw only what it declares; a
//! call to anything else, or to something that cannot be resolved, can throw
//! any exception.  Only a body that throws something, and declares all of
//! it, can rule a catch out, and only one whose class is unrelated to every
//! declared exception.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::walker::Walker;

use super::{ForwardWalkCtx, ScopeState};
use crate::atom::bytes_to_str;
use crate::class_lookup::Fqn;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ClassLikeKind};

/// What the statements of a `try` body can throw.
#[derive(Default)]
pub(crate) struct ThrowPoints {
    /// Exception classes a `throw` or a callee's `@throws` names.
    explicit: Vec<String>,
    /// Some statement may throw an exception nothing declares.
    implicit: bool,
}

impl ThrowPoints {
    /// Add what `stmt` can throw.
    ///
    /// `scope` is the state before the statement, used to find the class a
    /// variable receiver or a `throw $e` holds.  A statement that nests
    /// others can reassign the variable before the call runs, so the caller
    /// passes `None` for those and such a call counts as undeclared.
    pub(crate) fn collect(
        &mut self,
        stmt: &Statement<'_>,
        scope: Option<&ScopeState>,
        ctx: &ForwardWalkCtx<'_>,
    ) {
        if self.implicit {
            return;
        }
        let mut collect = Collect {
            ctx,
            scope,
            outer_scopes: Vec::new(),
            points: self,
        };
        walk_throw_sites(stmt, &mut collect);
    }

    /// Whether an exception the body throws can land in `catch`.
    pub(crate) fn reach(&self, catch: &TryCatchClause<'_>, ctx: &ForwardWalkCtx<'_>) -> bool {
        // A body that neither calls nor throws anything declares nothing to
        // rule the catch out against, so the catch is kept: whoever wrote
        // `try { $ok = true; } catch (...)` expects it to be able to run.
        if self.implicit || self.explicit.is_empty() {
            return true;
        }
        let hint = extract_hint_type(&catch.hint);
        hint.union_members().into_iter().any(|member| {
            // The name as written: `base_name` would drop the leading `\` of
            // `catch (\RuntimeException $e)`, turning it namespace-relative.
            let crate::php_type::TypeKind::Named(name) = member.kind() else {
                return true;
            };
            let catch_name = ctx.written_names().resolve_class(name);
            self.explicit
                .iter()
                .any(|thrown| may_catch(&catch_name, thrown, ctx))
        })
    }

    /// Whether anything was found that can throw.
    pub(crate) fn may_throw(&self) -> bool {
        self.implicit || !self.explicit.is_empty()
    }

    /// Add everything `other` can throw.
    pub(crate) fn absorb(&mut self, other: ThrowPoints) {
        if self.implicit {
            return;
        }
        self.implicit = other.implicit;
        for name in other.explicit {
            self.add_class(name);
        }
    }

    fn add_class(&mut self, name: String) {
        if !self.explicit.contains(&name) {
            self.explicit.push(name);
        }
    }

    /// Add what a callee declares it throws; one that declares nothing can
    /// throw anything.
    fn add_declared(&mut self, throws: &[PhpType]) {
        if throws.is_empty() {
            self.implicit = true;
            return;
        }
        for ty in throws {
            for member in ty.union_members() {
                if member.is_void() || member.is_never() {
                    continue;
                }
                match member.base_name() {
                    Some(name) => self.add_class(name.to_string()),
                    None => self.implicit = true,
                }
            }
        }
    }
}

/// Whether a `catch` of `catch_name` can take an exception declared as
/// `thrown`: one of the two classes extends the other.  An interface on
/// either side could be implemented by a subclass of the other, and a class
/// that cannot be loaded is unknown, so both count as a possible match.
fn may_catch(catch_name: &str, thrown: &str, ctx: &ForwardWalkCtx<'_>) -> bool {
    let Some(catch_class) = (ctx.file.class_loader)(Fqn::new(catch_name)) else {
        return true;
    };
    let Some(thrown_class) = load_fqn(thrown, ctx) else {
        return true;
    };
    if catch_class.kind == ClassLikeKind::Interface || thrown_class.kind == ClassLikeKind::Interface
    {
        return true;
    }
    crate::class_lookup::is_subtype_of(&thrown_class, &catch_class.fqn(), ctx.file.class_loader)
        || crate::class_lookup::is_subtype_of(
            &catch_class,
            &thrown_class.fqn(),
            ctx.file.class_loader,
        )
}

/// Load a class by the canonical name a `@throws` tag was resolved to.
///
/// A backslash-free name there is a global class, so it must not go
/// through the current file's `use` map the way a name typed in source does.
fn load_fqn(name: &str, ctx: &ForwardWalkCtx<'_>) -> Option<Arc<ClassInfo>> {
    (ctx.file.class_loader)(Fqn::new(name))
}

struct Collect<'c, 'w> {
    ctx: &'c ForwardWalkCtx<'w>,
    scope: Option<&'c ScopeState>,
    /// The scopes an immediately invoked closure's body set aside.
    outer_scopes: Vec<Option<&'c ScopeState>>,
    points: &'c mut ThrowPoints,
}

impl Collect<'_, '_> {
    /// The classes a receiver or thrown expression holds, or `None` when
    /// any of them is not a class this can name.
    fn classes_of(&self, expr: &Expression<'_>) -> Option<Vec<String>> {
        match expr {
            Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                Some(vec![self.ctx.current_class.fqn().to_string()])
            }
            Expression::Variable(Variable::Direct(dv)) => {
                let types = self.scope?.get(bytes_to_str(dv.name));
                if types.is_empty() {
                    return None;
                }
                types
                    .iter()
                    .map(|rt| {
                        let name = rt.type_string.base_name()?;
                        (!crate::php_type::is_primitive_scalar_name(name)).then(|| name.to_string())
                    })
                    .collect()
            }
            _ => None,
        }
    }

    /// Add what calling `method_name` on each of `classes` can throw.
    fn add_method_call(
        &mut self,
        classes: Option<Vec<String>>,
        method: &ClassLikeMemberSelector<'_>,
    ) {
        let ClassLikeMemberSelector::Identifier(ident) = method else {
            self.points.implicit = true;
            return;
        };
        let Some(classes) = classes else {
            self.points.implicit = true;
            return;
        };
        let method_name = bytes_to_str(ident.value);
        for class_name in classes {
            match self.merged_class(&class_name) {
                Some(merged) => match merged.get_method(method_name) {
                    Some(method) => self.points.add_declared(method.throws()),
                    None => self.points.implicit = true,
                },
                None => self.points.implicit = true,
            }
        }
    }

    fn merged_class(&self, class_name: &str) -> Option<Arc<ClassInfo>> {
        let cls = (self.ctx.file.class_loader)(Fqn::new(class_name))?;
        Some(crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            self.ctx.file.class_loader,
            self.ctx.resolved_class_cache,
        ))
    }

    fn class_name(&self, expr: &Expression<'_>) -> Option<String> {
        crate::class_lookup::class_expression_name(
            expr,
            self.ctx.current_class,
            self.ctx.file.all_classes,
            self.ctx.file.source_class_loader,
            self.ctx.file.source_imports,
        )
    }
}

/// Where the code of a statement can raise an exception, as reported by
/// [`walk_throw_sites`]: everything that runs where the statement is
/// written, and nothing that only runs once something calls it.
pub(crate) trait ThrowSiteSink<'ast, 'arena> {
    fn function_call(&mut self, call: &'ast FunctionCall<'arena>);
    fn method_call(
        &mut self,
        object: &'ast Expression<'arena>,
        method: &'ast ClassLikeMemberSelector<'arena>,
    );
    fn static_method_call(&mut self, call: &'ast StaticMethodCall<'arena>);
    fn instantiation(&mut self, new: &'ast Instantiation<'arena>);
    fn throw(&mut self, throw: &'ast Throw<'arena>);
    /// Code that can raise any exception at all: an anonymous class, the
    /// `yield` a generator can be handed an exception at, an `include`.
    fn any_exception(&mut self);
    /// The body of an immediately invoked closure starts: it has variables
    /// of its own, so none of the enclosing scope applies inside it.
    fn enter_invoked_closure(&mut self) {}
    fn leave_invoked_closure(&mut self) {}
    /// A `try` statement, before its blocks are walked.
    fn try_statement(&mut self, _try: &'ast Try<'arena>) {}
}

/// Report to `sink` where the code of `statement` can raise an exception.
///
/// Code in a closure, a nested declaration or an anonymous class body does
/// not run where it is written; calling it is the throw point.  An
/// immediately invoked closure runs on the spot, so its body is walked.
pub(crate) fn walk_throw_sites<'ast, 'arena, S: ThrowSiteSink<'ast, 'arena>>(
    statement: &'ast Statement<'arena>,
    sink: &mut S,
) {
    ThrowSiteWalker.walk_statement(statement, sink);
}

impl<'ast, 'arena> ThrowSiteSink<'ast, 'arena> for Collect<'_, '_> {
    fn function_call(&mut self, call: &'ast FunctionCall<'arena>) {
        let (Expression::Identifier(ident), Some(fl)) =
            (call.function, self.ctx.loaders.function_loader)
        else {
            self.points.implicit = true;
            return;
        };
        match fl(
            bytes_to_str(ident.value()),
            call.function.span().start.offset,
        ) {
            Some(func) => self.points.add_declared(&func.throws),
            None => self.points.implicit = true,
        }
    }

    fn method_call(
        &mut self,
        object: &'ast Expression<'arena>,
        method: &'ast ClassLikeMemberSelector<'arena>,
    ) {
        let classes = self.classes_of(object);
        self.add_method_call(classes, method);
    }

    fn static_method_call(&mut self, call: &'ast StaticMethodCall<'arena>) {
        let classes = self.class_name(call.class).map(|name| vec![name]);
        self.add_method_call(classes, &call.method);
    }

    fn instantiation(&mut self, new: &'ast Instantiation<'arena>) {
        let Some(merged) = self
            .class_name(new.class)
            .and_then(|name| self.merged_class(&name))
        else {
            self.points.implicit = true;
            return;
        };
        let Some(constructor) = merged.get_method("__construct") else {
            return;
        };
        // An exception's constructor only stores its arguments, which is
        // what lets `throw new Foo()` throw just `Foo`.
        if constructor.throws().is_empty()
            && crate::class_lookup::is_subtype_of(&merged, "Throwable", self.ctx.file.class_loader)
        {
            return;
        }
        self.points.add_declared(constructor.throws());
    }

    fn throw(&mut self, throw: &'ast Throw<'arena>) {
        let classes = match throw.exception {
            Expression::Instantiation(new) => self.class_name(new.class).map(|name| vec![name]),
            other => self.classes_of(other),
        };
        match classes {
            Some(classes) => {
                for name in classes {
                    match (self.ctx.file.class_loader)(Fqn::new(&name)) {
                        Some(cls) => self.points.add_class(cls.fqn().to_string()),
                        None => self.points.implicit = true,
                    }
                }
            }
            None => self.points.implicit = true,
        }
    }

    fn any_exception(&mut self) {
        self.points.implicit = true;
    }

    fn enter_invoked_closure(&mut self) {
        self.outer_scopes.push(self.scope.take());
    }

    fn leave_invoked_closure(&mut self) {
        self.scope = self.outer_scopes.pop().flatten();
    }
}

struct ThrowSiteWalker;

impl<'ast, 'arena, S: ThrowSiteSink<'ast, 'arena>> Walker<'ast, 'arena, S> for ThrowSiteWalker {
    fn walk_closure(&self, _: &'ast Closure<'arena>, _: &mut S) {}
    fn walk_arrow_function(&self, _: &'ast ArrowFunction<'arena>, _: &mut S) {}
    fn walk_function(&self, _: &'ast Function<'arena>, _: &mut S) {}
    fn walk_class(&self, _: &'ast Class<'arena>, _: &mut S) {}
    fn walk_interface(&self, _: &'ast Interface<'arena>, _: &mut S) {}
    fn walk_trait(&self, _: &'ast Trait<'arena>, _: &mut S) {}
    fn walk_enum(&self, _: &'ast Enum<'arena>, _: &mut S) {}

    fn walk_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, sink: &mut S) {
        sink.any_exception();
    }

    fn walk_in_function_call(&self, call: &'ast FunctionCall<'arena>, sink: &mut S) {
        let mut callee = call.function;
        while let Expression::Parenthesized(p) = callee {
            callee = p.expression;
        }
        // An immediately invoked closure throws what its body throws.
        match callee {
            Expression::Closure(closure) => {
                sink.enter_invoked_closure();
                for stmt in closure.body.statements.iter() {
                    self.walk_statement(stmt, sink);
                }
                sink.leave_invoked_closure();
            }
            Expression::ArrowFunction(arrow) => {
                sink.enter_invoked_closure();
                self.walk_expression(arrow.expression, sink);
                sink.leave_invoked_closure();
            }
            _ => sink.function_call(call),
        }
    }

    fn walk_in_method_call(&self, call: &'ast MethodCall<'arena>, sink: &mut S) {
        sink.method_call(call.object, &call.method);
    }

    fn walk_in_null_safe_method_call(&self, call: &'ast NullSafeMethodCall<'arena>, sink: &mut S) {
        sink.method_call(call.object, &call.method);
    }

    fn walk_in_static_method_call(&self, call: &'ast StaticMethodCall<'arena>, sink: &mut S) {
        sink.static_method_call(call);
    }

    fn walk_in_instantiation(&self, new: &'ast Instantiation<'arena>, sink: &mut S) {
        sink.instantiation(new);
    }

    fn walk_in_throw(&self, throw: &'ast Throw<'arena>, sink: &mut S) {
        sink.throw(throw);
    }

    fn walk_in_try(&self, try_statement: &'ast Try<'arena>, sink: &mut S) {
        sink.try_statement(try_statement);
    }

    fn walk_in_yield(&self, _: &'ast Yield<'arena>, sink: &mut S) {
        sink.any_exception();
    }

    fn walk_in_construct(&self, construct: &'ast Construct<'arena>, sink: &mut S) {
        if matches!(
            construct,
            Construct::Include(_)
                | Construct::IncludeOnce(_)
                | Construct::Require(_)
                | Construct::RequireOnce(_)
        ) {
            sink.any_exception();
        }
    }
}
