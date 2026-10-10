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
            points: self,
        };
        ThrowPointWalker.walk_statement(stmt, &mut collect);
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
            let Some(name) = member.base_name() else {
                return true;
            };
            let catch_name = crate::util::resolve_source_class_name(
                name,
                ctx.current_class.file_namespace.as_deref(),
                ctx.all_classes,
                ctx.class_loader,
            );
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
    let Some(catch_class) = (ctx.class_loader)(catch_name) else {
        return true;
    };
    let Some(thrown_class) = load_fqn(thrown, ctx) else {
        return true;
    };
    if catch_class.kind == ClassLikeKind::Interface || thrown_class.kind == ClassLikeKind::Interface
    {
        return true;
    }
    crate::class_lookup::is_subtype_of(&thrown_class, &catch_class.fqn(), ctx.class_loader)
        || crate::class_lookup::is_subtype_of(&catch_class, &thrown_class.fqn(), ctx.class_loader)
}

/// Load a class by the canonical name a `@throws` tag was resolved to.
///
/// A backslash-free name there is a global class, so it must not go
/// through the current file's `use` map the way a name typed in source does.
fn load_fqn(name: &str, ctx: &ForwardWalkCtx<'_>) -> Option<Arc<ClassInfo>> {
    if name.contains('\\') {
        (ctx.class_loader)(name)
    } else {
        (ctx.class_loader)(&format!("__fqn__\\{name}")).or_else(|| (ctx.class_loader)(name))
    }
}

struct Collect<'c, 'w> {
    ctx: &'c ForwardWalkCtx<'w>,
    scope: Option<&'c ScopeState>,
    points: &'c mut ThrowPoints,
}

impl Collect<'_, '_> {
    /// The classes a receiver or thrown expression holds, or `None` when
    /// any of them is not a class this can name.
    fn classes_of(&self, expr: &Expression<'_>) -> Option<Vec<String>> {
        match expr {
            Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                Some(vec![self.ctx.current_class.name.to_string()])
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
                    Some(method) => self.points.add_declared(&method.throws),
                    None => self.points.implicit = true,
                },
                None => self.points.implicit = true,
            }
        }
    }

    fn merged_class(&self, class_name: &str) -> Option<Arc<ClassInfo>> {
        let cls = (self.ctx.class_loader)(class_name)?;
        Some(crate::virtual_members::resolve_class_fully_maybe_cached(
            &cls,
            self.ctx.class_loader,
            self.ctx.resolved_class_cache,
        ))
    }

    fn class_name(&self, expr: &Expression<'_>) -> Option<String> {
        crate::class_lookup::class_expression_name(
            expr,
            self.ctx.current_class,
            self.ctx.all_classes,
            self.ctx.class_loader,
        )
    }
}

struct ThrowPointWalker;

impl<'ast, 'arena, 'c, 'w> Walker<'ast, 'arena, Collect<'c, 'w>> for ThrowPointWalker {
    // Code in a closure, a nested declaration or an anonymous class body
    // does not run where it is written; calling it is the throw point.
    fn walk_closure(&self, _: &'ast Closure<'arena>, _: &mut Collect<'c, 'w>) {}
    fn walk_arrow_function(&self, _: &'ast ArrowFunction<'arena>, _: &mut Collect<'c, 'w>) {}
    fn walk_function(&self, _: &'ast Function<'arena>, _: &mut Collect<'c, 'w>) {}
    fn walk_class(&self, _: &'ast Class<'arena>, _: &mut Collect<'c, 'w>) {}
    fn walk_interface(&self, _: &'ast Interface<'arena>, _: &mut Collect<'c, 'w>) {}
    fn walk_trait(&self, _: &'ast Trait<'arena>, _: &mut Collect<'c, 'w>) {}
    fn walk_enum(&self, _: &'ast Enum<'arena>, _: &mut Collect<'c, 'w>) {}

    fn walk_anonymous_class(&self, _: &'ast AnonymousClass<'arena>, c: &mut Collect<'c, 'w>) {
        c.points.implicit = true;
    }

    fn walk_in_function_call(&self, call: &'ast FunctionCall<'arena>, c: &mut Collect<'c, 'w>) {
        let mut callee = call.function;
        while let Expression::Parenthesized(p) = callee {
            callee = p.expression;
        }
        // An immediately invoked closure throws what its body throws.  The
        // body has its own variables, so none of the outer scope applies.
        match callee {
            Expression::Closure(closure) => {
                let outer = c.scope.take();
                for stmt in closure.body.statements.iter() {
                    self.walk_statement(stmt, c);
                }
                c.scope = outer;
                return;
            }
            Expression::ArrowFunction(arrow) => {
                let outer = c.scope.take();
                self.walk_expression(arrow.expression, c);
                c.scope = outer;
                return;
            }
            _ => {}
        }
        let (Expression::Identifier(ident), Some(fl)) =
            (call.function, c.ctx.loaders.function_loader)
        else {
            c.points.implicit = true;
            return;
        };
        match fl(
            bytes_to_str(ident.value()),
            call.function.span().start.offset,
        ) {
            Some(func) => c.points.add_declared(&func.throws),
            None => c.points.implicit = true,
        }
    }

    fn walk_in_method_call(&self, call: &'ast MethodCall<'arena>, c: &mut Collect<'c, 'w>) {
        let classes = c.classes_of(call.object);
        c.add_method_call(classes, &call.method);
    }

    fn walk_in_null_safe_method_call(
        &self,
        call: &'ast NullSafeMethodCall<'arena>,
        c: &mut Collect<'c, 'w>,
    ) {
        let classes = c.classes_of(call.object);
        c.add_method_call(classes, &call.method);
    }

    fn walk_in_static_method_call(
        &self,
        call: &'ast StaticMethodCall<'arena>,
        c: &mut Collect<'c, 'w>,
    ) {
        let classes = c.class_name(call.class).map(|name| vec![name]);
        c.add_method_call(classes, &call.method);
    }

    fn walk_in_instantiation(&self, new: &'ast Instantiation<'arena>, c: &mut Collect<'c, 'w>) {
        let Some(merged) = c
            .class_name(new.class)
            .and_then(|name| c.merged_class(&name))
        else {
            c.points.implicit = true;
            return;
        };
        let Some(constructor) = merged.get_method("__construct") else {
            return;
        };
        // An exception's constructor only stores its arguments, which is
        // what lets `throw new Foo()` throw just `Foo`.
        if constructor.throws.is_empty()
            && crate::class_lookup::is_subtype_of(&merged, "Throwable", c.ctx.class_loader)
        {
            return;
        }
        c.points.add_declared(&constructor.throws);
    }

    fn walk_in_throw(&self, throw: &'ast Throw<'arena>, c: &mut Collect<'c, 'w>) {
        let classes = match throw.exception {
            Expression::Instantiation(new) => c.class_name(new.class).map(|name| vec![name]),
            other => c.classes_of(other),
        };
        match classes {
            Some(classes) => {
                for name in classes {
                    match (c.ctx.class_loader)(&name) {
                        Some(cls) => c.points.add_class(cls.fqn().to_string()),
                        None => c.points.implicit = true,
                    }
                }
            }
            None => c.points.implicit = true,
        }
    }

    // A generator can be handed any exception at the `yield` it paused on.
    fn walk_in_yield(&self, _: &'ast Yield<'arena>, c: &mut Collect<'c, 'w>) {
        c.points.implicit = true;
    }

    fn walk_in_construct(&self, construct: &'ast Construct<'arena>, c: &mut Collect<'c, 'w>) {
        if matches!(
            construct,
            Construct::Include(_)
                | Construct::IncludeOnce(_)
                | Construct::Require(_)
                | Construct::RequireOnce(_)
        ) {
            c.points.implicit = true;
        }
    }
}
