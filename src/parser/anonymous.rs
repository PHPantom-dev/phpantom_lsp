//! Extraction of anonymous classes (`new class { ... }`).
//!
//! Anonymous classes are given synthetic names of the form
//! `__anonymous@<offset>` so that
//! [`find_class_at_offset`](crate::util::find_class_at_offset) can resolve
//! `$this` inside their bodies. This module walks statement and expression
//! trees looking for `Expression::AnonymousClass` nodes, recursing into
//! method bodies (including nested anonymous classes) along the way.

use std::sync::Arc;

use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom, atom_bytes};
use crate::types::*;

use super::DocblockCtx;

impl Backend {
    /// Build a [`ClassInfo`] for an anonymous class expression.
    fn extract_anonymous_class_info<'a>(
        anon: &AnonymousClass<'a>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) -> ClassInfo {
        let parent_class = anon
            .extends
            .as_ref()
            .and_then(|ext| ext.types.first().map(|ident| atom_bytes(ident.value())));

        let interfaces: Vec<Atom> = anon
            .implements
            .as_ref()
            .map(|imp| {
                imp.types
                    .iter()
                    .map(|ident| atom_bytes(ident.value()))
                    .collect()
            })
            .unwrap_or_default();

        let ExtractedMembers {
            methods,
            properties,
            constants,
            used_traits,
            trait_precedences,
            trait_aliases,
            ..
        } = Self::extract_class_like_members(anon.members.iter(), doc_ctx, &[]);

        let start_offset = anon.left_brace.start.offset;
        let end_offset = anon.right_brace.end.offset;
        // Anonymous classes don't have a meaningful keyword_offset for
        // go-to-definition purposes — use 0 ("not available").
        let keyword_offset = 0;
        let name = atom(&format!("__anonymous@{}", start_offset));

        ClassInfo {
            kind: ClassLikeKind::Class,
            name,
            methods: methods.into_iter().map(Arc::new).collect::<Vec<_>>().into(),
            properties: properties.into(),
            constants: constants.into(),
            start_offset,
            end_offset,
            keyword_offset,
            decl_start_offset: start_offset,
            parent_class,
            interfaces,
            used_traits,
            mixins: vec![],
            mixin_generics: vec![],
            require_extends: None,
            require_implements: Vec::new(),
            is_final: false,
            is_abstract: false,
            deprecation_message: None,
            deprecated_replacement: None,
            template_params: vec![],
            template_param_bounds: AtomMap::default(),
            template_param_defaults: AtomMap::default(),
            extends_generics: vec![],
            implements_generics: vec![],
            use_generics: vec![],
            type_aliases: AtomMap::default(),
            trait_precedences,
            trait_aliases,
            links: Vec::new(),
            see_refs: Vec::new(),
            class_docblock: None,
            file_namespace: None,
            backed_type: None,
            attribute_targets: 0,
            method_index: Default::default(),
            indexed_method_count: 0,
            laravel: None,
        }
    }

    /// Recursively walk a statement looking for anonymous classes in
    /// expressions and nested statement blocks.
    pub(crate) fn find_anonymous_classes_in_statement<'a>(
        statement: &'a Statement<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match statement {
            Statement::Expression(expr_stmt) => {
                Self::find_anonymous_classes_in_expression(expr_stmt.expression, classes, doc_ctx);
            }
            Statement::Return(ret) => {
                if let Some(value) = &ret.value {
                    Self::find_anonymous_classes_in_expression(value, classes, doc_ctx);
                }
            }
            Statement::Block(block) => {
                Self::walk_statements_for_anonymous_classes(
                    block.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
            Statement::If(if_stmt) => {
                Self::find_anonymous_classes_in_if_body(&if_stmt.body, classes, doc_ctx);
            }
            Statement::While(while_stmt) => match &while_stmt.body {
                WhileBody::Statement(stmt) => {
                    Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
                }
                WhileBody::ColonDelimited(body) => {
                    Self::walk_statements_for_anonymous_classes(
                        body.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            },
            Statement::DoWhile(do_while) => {
                Self::find_anonymous_classes_in_statement(do_while.statement, classes, doc_ctx);
            }
            Statement::For(for_stmt) => match &for_stmt.body {
                ForBody::Statement(stmt) => {
                    Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
                }
                ForBody::ColonDelimited(body) => {
                    Self::walk_statements_for_anonymous_classes(
                        body.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            },
            Statement::Foreach(foreach_stmt) => match &foreach_stmt.body {
                ForeachBody::Statement(stmt) => {
                    Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
                }
                ForeachBody::ColonDelimited(body) => {
                    Self::walk_statements_for_anonymous_classes(
                        body.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            },
            Statement::Switch(switch_stmt) => {
                let cases = match &switch_stmt.body {
                    SwitchBody::BraceDelimited(b) => &b.cases,
                    SwitchBody::ColonDelimited(b) => &b.cases,
                };
                for case in cases.iter() {
                    let stmts = match case {
                        SwitchCase::Expression(c) => &c.statements,
                        SwitchCase::Default(c) => &c.statements,
                    };
                    Self::walk_statements_for_anonymous_classes(stmts.iter(), classes, doc_ctx);
                }
            }
            Statement::Try(try_stmt) => {
                Self::walk_statements_for_anonymous_classes(
                    try_stmt.block.statements.iter(),
                    classes,
                    doc_ctx,
                );
                for catch in try_stmt.catch_clauses.iter() {
                    Self::walk_statements_for_anonymous_classes(
                        catch.block.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                if let Some(finally) = &try_stmt.finally_clause {
                    Self::walk_statements_for_anonymous_classes(
                        finally.block.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            }
            Statement::Function(func) => {
                Self::walk_statements_for_anonymous_classes(
                    func.body.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
            // Named class-like declarations: walk method bodies to find
            // anonymous classes used inside methods.
            Statement::Class(class) => {
                Self::find_anonymous_classes_in_members(class.members.iter(), classes, doc_ctx);
            }
            Statement::Interface(iface) => {
                Self::find_anonymous_classes_in_members(iface.members.iter(), classes, doc_ctx);
            }
            Statement::Trait(trait_def) => {
                Self::find_anonymous_classes_in_members(trait_def.members.iter(), classes, doc_ctx);
            }
            Statement::Enum(enum_def) => {
                Self::find_anonymous_classes_in_members(enum_def.members.iter(), classes, doc_ctx);
            }
            Statement::Namespace(ns) => {
                Self::walk_statements_for_anonymous_classes(
                    ns.statements().iter(),
                    classes,
                    doc_ctx,
                );
            }
            Statement::Echo(echo) => {
                for expr in echo.values.iter() {
                    Self::find_anonymous_classes_in_expression(expr, classes, doc_ctx);
                }
            }
            _ => {}
        }
    }

    /// Walk class-like member method bodies to find anonymous classes.
    pub(super) fn find_anonymous_classes_in_members<'a>(
        members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for member in members {
            if let ClassLikeMember::Method(method) = member
                && let MethodBody::Concrete(block) = &method.body
            {
                Self::walk_statements_for_anonymous_classes(
                    block.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
        }
    }

    /// Walk a sequence of statements, dispatching each to the
    /// anonymous-class finder.
    fn walk_statements_for_anonymous_classes<'a>(
        statements: impl Iterator<Item = &'a Statement<'a>>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for stmt in statements {
            Self::find_anonymous_classes_in_statement(stmt, classes, doc_ctx);
        }
    }

    /// Helper: recurse into an `if` statement body for anonymous classes.
    fn find_anonymous_classes_in_if_body<'a>(
        body: &'a IfBody<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match body {
            IfBody::Statement(body) => {
                Self::find_anonymous_classes_in_statement(body.statement, classes, doc_ctx);
                for else_if in body.else_if_clauses.iter() {
                    Self::find_anonymous_classes_in_statement(else_if.statement, classes, doc_ctx);
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::find_anonymous_classes_in_statement(
                        else_clause.statement,
                        classes,
                        doc_ctx,
                    );
                }
            }
            IfBody::ColonDelimited(body) => {
                Self::walk_statements_for_anonymous_classes(
                    body.statements.iter(),
                    classes,
                    doc_ctx,
                );
                for else_if in body.else_if_clauses.iter() {
                    Self::walk_statements_for_anonymous_classes(
                        else_if.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
                if let Some(else_clause) = &body.else_clause {
                    Self::walk_statements_for_anonymous_classes(
                        else_clause.statements.iter(),
                        classes,
                        doc_ctx,
                    );
                }
            }
        }
    }

    /// Recursively walk an expression tree looking for
    /// `Expression::AnonymousClass` nodes.
    fn find_anonymous_classes_in_expression<'a>(
        expr: &'a Expression<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match expr {
            Expression::AnonymousClass(anon) => {
                let info = Self::extract_anonymous_class_info(anon, doc_ctx);
                classes.push(info);
                // Also recurse into the anonymous class's method bodies
                // to find nested anonymous classes.
                Self::find_anonymous_classes_in_members(anon.members.iter(), classes, doc_ctx);
            }
            Expression::Assignment(assignment) => {
                Self::find_anonymous_classes_in_expression(assignment.lhs, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(assignment.rhs, classes, doc_ctx);
            }
            Expression::Parenthesized(paren) => {
                Self::find_anonymous_classes_in_expression(paren.expression, classes, doc_ctx);
            }
            Expression::Binary(binary) => {
                Self::find_anonymous_classes_in_expression(binary.lhs, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(binary.rhs, classes, doc_ctx);
            }
            Expression::UnaryPrefix(unary) => {
                Self::find_anonymous_classes_in_expression(unary.operand, classes, doc_ctx);
            }
            Expression::UnaryPostfix(unary) => {
                Self::find_anonymous_classes_in_expression(unary.operand, classes, doc_ctx);
            }
            Expression::Conditional(cond) => {
                Self::find_anonymous_classes_in_expression(cond.condition, classes, doc_ctx);
                if let Some(then) = &cond.then {
                    Self::find_anonymous_classes_in_expression(then, classes, doc_ctx);
                }
                Self::find_anonymous_classes_in_expression(cond.r#else, classes, doc_ctx);
            }
            Expression::Call(call) => {
                Self::find_anonymous_classes_in_argument_list(
                    call.get_argument_list(),
                    classes,
                    doc_ctx,
                );
                // Also walk the object/class/function expression
                match call {
                    Call::Function(fc) => {
                        Self::find_anonymous_classes_in_expression(fc.function, classes, doc_ctx);
                    }
                    Call::Method(mc) => {
                        Self::find_anonymous_classes_in_expression(mc.object, classes, doc_ctx);
                    }
                    Call::NullSafeMethod(nmc) => {
                        Self::find_anonymous_classes_in_expression(nmc.object, classes, doc_ctx);
                    }
                    Call::StaticMethod(smc) => {
                        Self::find_anonymous_classes_in_expression(smc.class, classes, doc_ctx);
                    }
                }
            }
            Expression::Instantiation(inst) => {
                Self::find_anonymous_classes_in_expression(inst.class, classes, doc_ctx);
                if let Some(args) = &inst.argument_list {
                    Self::find_anonymous_classes_in_argument_list(args, classes, doc_ctx);
                }
            }
            Expression::Throw(throw) => {
                Self::find_anonymous_classes_in_expression(throw.exception, classes, doc_ctx);
            }
            Expression::Clone(clone) => {
                Self::find_anonymous_classes_in_expression(clone.object, classes, doc_ctx);
            }
            Expression::Yield(yld) => match yld {
                Yield::Value(yv) => {
                    if let Some(value) = &yv.value {
                        Self::find_anonymous_classes_in_expression(value, classes, doc_ctx);
                    }
                }
                Yield::Pair(yp) => {
                    Self::find_anonymous_classes_in_expression(yp.key, classes, doc_ctx);
                    Self::find_anonymous_classes_in_expression(yp.value, classes, doc_ctx);
                }
                Yield::From(yf) => {
                    Self::find_anonymous_classes_in_expression(yf.iterator, classes, doc_ctx);
                }
            },
            Expression::Match(match_expr) => {
                Self::find_anonymous_classes_in_expression(match_expr.expression, classes, doc_ctx);
                for arm in match_expr.arms.iter() {
                    let arm_expr = arm.expression();
                    Self::find_anonymous_classes_in_expression(arm_expr, classes, doc_ctx);
                }
            }
            Expression::Array(array) => {
                for element in array.elements.iter() {
                    Self::find_anonymous_classes_in_array_element(element, classes, doc_ctx);
                }
            }
            Expression::LegacyArray(array) => {
                for element in array.elements.iter() {
                    Self::find_anonymous_classes_in_array_element(element, classes, doc_ctx);
                }
            }
            Expression::ArrayAccess(access) => {
                Self::find_anonymous_classes_in_expression(access.array, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(access.index, classes, doc_ctx);
            }
            Expression::Access(access) => match access {
                Access::Property(pa) => {
                    Self::find_anonymous_classes_in_expression(pa.object, classes, doc_ctx);
                }
                Access::NullSafeProperty(npa) => {
                    Self::find_anonymous_classes_in_expression(npa.object, classes, doc_ctx);
                }
                Access::StaticProperty(spa) => {
                    Self::find_anonymous_classes_in_expression(spa.class, classes, doc_ctx);
                }
                Access::ClassConstant(cca) => {
                    Self::find_anonymous_classes_in_expression(cca.class, classes, doc_ctx);
                }
            },
            Expression::Closure(closure) => {
                Self::walk_statements_for_anonymous_classes(
                    closure.body.statements.iter(),
                    classes,
                    doc_ctx,
                );
            }
            Expression::ArrowFunction(arrow) => {
                Self::find_anonymous_classes_in_expression(arrow.expression, classes, doc_ctx);
            }
            // Terminal expressions that cannot contain anonymous classes.
            Expression::Literal(_)
            | Expression::Variable(_)
            | Expression::Identifier(_)
            | Expression::ConstantAccess(_)
            | Expression::MagicConstant(_)
            | Expression::Parent(_)
            | Expression::Static(_)
            | Expression::Self_(_)
            | Expression::Error(_) => {}
            // Catch-all for less common expression types (Construct,
            // CompositeString, List, Pipe, ArrayAppend, PartialApplication).
            // These rarely contain anonymous classes, but if they do,
            // we'll miss them — acceptable for a first implementation.
            _ => {}
        }
    }

    /// Walk an argument list to find anonymous classes in argument values.
    fn find_anonymous_classes_in_argument_list<'a>(
        args: &'a ArgumentList<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        for arg in args.arguments.iter() {
            let expr = match arg {
                Argument::Positional(pos) => pos.value,
                Argument::Named(named) => named.value,
            };
            Self::find_anonymous_classes_in_expression(expr, classes, doc_ctx);
        }
    }

    /// Walk an array element to find anonymous classes in values/keys.
    fn find_anonymous_classes_in_array_element<'a>(
        element: &'a ArrayElement<'a>,
        classes: &mut Vec<ClassInfo>,
        doc_ctx: Option<&DocblockCtx<'a>>,
    ) {
        match element {
            ArrayElement::KeyValue(kv) => {
                Self::find_anonymous_classes_in_expression(kv.key, classes, doc_ctx);
                Self::find_anonymous_classes_in_expression(kv.value, classes, doc_ctx);
            }
            ArrayElement::Value(v) => {
                Self::find_anonymous_classes_in_expression(v.value, classes, doc_ctx);
            }
            ArrayElement::Variadic(v) => {
                Self::find_anonymous_classes_in_expression(v.value, classes, doc_ctx);
            }
            ArrayElement::Missing(_) => {}
        }
    }
}
