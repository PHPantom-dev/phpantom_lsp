//! The views Yii renders, as PHP names them (see [`crate::yii::views`]).
//!
//! A view renderer's `render()` family and a view's own `render()` are far
//! too common to claim on sight, so a call that names a view by name is
//! recorded as a [`TypedReceiverSite`] for the type engine to confirm.  What
//! the name is relative to is read where the call is written: the view path
//! a class gives its renderer with `withViewPath()`, after the directory
//! `withController()` or `withControllerName()` adds, or, in a template, the
//! template's own directory.  A view named by a path built from `__DIR__`
//! names its file whatever renders it, so that call names a view on sight,
//! which an application's own wrapper around the renderer passes through as
//! well.

use std::path::Path;

use mago_span::HasSpan;
use mago_syntax::walker::Walker;

use super::*;
use crate::document_links::try_evaluate_path_expr;
use crate::symbol_map::{ReceiverClass, StringKeyKind};
use crate::virtual_members::laravel::helpers::resolved_string_literal;
use crate::yii::views::{controller_name, is_view_render_method, normalize_view_path, view_key};

/// Where the render calls being extracted are written, which says what a
/// view name in them is relative to.
#[derive(Debug, Clone, Default)]
pub(in crate::symbol_map::extraction) enum YiiViewScope {
    /// Code outside any class: a template, whose `render()` names a view
    /// relative to its own directory.
    #[default]
    Template,
    /// A class, with the view path it gives its renderer, when it gives one
    /// that can be read.
    Class(Option<String>),
}

/// The scope the members of `class` are extracted in.
pub(in crate::symbol_map::extraction) fn yii_class_scope(
    class: &Class<'_>,
    ctx: &ExtractionCtx<'_>,
) -> YiiViewScope {
    let Some(dir) = ctx.yii_dir else {
        return YiiViewScope::Class(None);
    };
    let class_fqn = ctx.resolved_name_at(class.name.span.start.offset);
    YiiViewScope::Class(class_view_base(class, ctx.content, dir, class_fqn))
}

/// The view path `class`, written in a file in `dir`, gives its renderer:
/// the one every `withViewPath()` in it gives, after the directory a
/// `withController()` or `withControllerName()` adds.  `class_fqn` names
/// the controller `withController($this)` derives the directory from.
pub(crate) fn class_view_base(
    class: &Class<'_>,
    content: &str,
    dir: &Path,
    class_fqn: Option<&str>,
) -> Option<String> {
    let span = class.span();
    let text = content
        .as_bytes()
        .get(span.start.offset as usize..span.end.offset as usize)
        .unwrap_or_default();
    memchr::memmem::find(text, b"withViewPath")?;
    let mut calls = ViewPathCalls::default();
    let walker = ViewPathWalker { dir };
    for member in class.members.iter() {
        walker.walk_class_like_member(member, &mut calls);
    }
    calls.base(class_fqn)
}

/// Record the view the first argument of a render call names.
pub(in crate::symbol_map::extraction) fn record_yii_view_sites(
    object: &Expression<'_>,
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(dir) = ctx.yii_dir else {
        return;
    };
    if !is_view_render_method(member_name) {
        return;
    }
    let Some(Argument::Positional(argument)) = argument_list.arguments.first() else {
        return;
    };
    if let Expression::Binary(binary) = argument.value
        && binary.operator.is_concatenation()
    {
        let Some(path) = try_evaluate_path_expr(argument.value, dir) else {
            return;
        };
        if !path.starts_with('/') {
            return;
        }
        let Some((_, start, end)) = resolved_string_literal(binary.rhs, ctx.content) else {
            return;
        };
        ctx.spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                key: normalize_view_path(&path),
                kind: StringKeyKind::YiiView,
                is_write: false,
                is_optional: false,
            },
        });
        return;
    }
    let Some((name, start, end)) = resolved_string_literal(argument.value, ctx.content) else {
        return;
    };
    // A Twig `render()` already claimed the string for the type engine.
    if ctx
        .typed_receiver_sites
        .last()
        .is_some_and(|site| site.start == start as u32)
    {
        return;
    }
    let key = match &ctx.yii_view_scope {
        YiiViewScope::Template => {
            let dir = dir.to_string_lossy();
            view_key(name, Some(&dir), Some(&dir))
        }
        YiiViewScope::Class(base) => {
            let class_fqn = ctx
                .yii_class_name_offset
                .and_then(|offset| ctx.resolved_name_at(offset));
            match chain_view_base(object, dir, class_fqn) {
                Some(base) => view_key(name, base.as_deref(), None),
                None => view_key(name, base.as_deref(), None),
            }
        }
    };
    let Some(key) = key else {
        return;
    };
    ctx.typed_receiver_sites.push(TypedReceiverSite {
        start: start as u32,
        end: end as u32,
        key,
        is_write: false,
        is_optional: false,
        receiver: ReceiverClass::YiiViewRenderer,
    });
}

/// The view path a renderer built in the receiver chain itself is given,
/// `$renderer->withViewPath('@views/blog')->render('index')`, or `None`
/// when the chain does not give one.
pub(crate) fn chain_view_base(
    object: &Expression<'_>,
    dir: &Path,
    class_fqn: Option<&str>,
) -> Option<Option<String>> {
    let mut calls = ViewPathCalls::default();
    let mut current = object;
    while let Expression::Call(Call::Method(call)) = current {
        if let ClassLikeMemberSelector::Identifier(method) = &call.method {
            calls.record(bytes_to_str(method.value), &call.argument_list, dir);
        }
        current = call.object;
    }
    if calls.paths.is_empty() {
        return None;
    }
    Some(calls.base(class_fqn))
}

/// What a class's or a chain's `with…()` calls give its renderer.
#[derive(Default)]
struct ViewPathCalls {
    /// The view path of each `withViewPath()`, `None` for one that cannot
    /// be read.
    paths: Vec<Option<String>>,
    /// The directory `withController()` or `withControllerName()` adds.
    controller: Option<ControllerDir>,
}

enum ControllerDir {
    /// `withController($this)`: the class's own controller name.
    This,
    /// `withControllerName('blog')`.
    Named(String),
    /// A controller or a name that cannot be read.
    Unknown,
}

impl ViewPathCalls {
    fn record(&mut self, method: &str, arguments: &ArgumentList<'_>, dir: &Path) {
        let first = match arguments.arguments.first() {
            Some(Argument::Positional(argument)) => Some(argument.value),
            Some(Argument::Named(argument)) => Some(argument.value),
            None => None,
        };
        if method.eq_ignore_ascii_case("withViewPath") {
            self.paths
                .push(first.and_then(|value| view_path_value(value, dir)));
        } else if method.eq_ignore_ascii_case("withController") {
            let is_this = matches!(
                first,
                Some(Expression::Variable(Variable::Direct(variable))) if variable.name == b"$this"
            );
            self.controller = Some(if is_this {
                ControllerDir::This
            } else {
                ControllerDir::Unknown
            });
        } else if method.eq_ignore_ascii_case("withControllerName") {
            let name = match first {
                Some(Expression::Literal(Literal::String(literal))) => {
                    literal.value.and_then(crate::atom::literal_bytes_to_str)
                }
                _ => None,
            };
            self.controller = Some(match name {
                Some(name) => ControllerDir::Named(name.to_string()),
                None => ControllerDir::Unknown,
            });
        }
    }

    /// The view path the calls give, when they agree on one that can be
    /// read.
    fn base(self, class_fqn: Option<&str>) -> Option<String> {
        let mut paths = self.paths.into_iter();
        let first = paths.next()??;
        for path in paths {
            if path.as_ref() != Some(&first) {
                return None;
            }
        }
        match self.controller {
            None => Some(first),
            Some(ControllerDir::This) => Some(format!("{first}/{}", controller_name(class_fqn?)?)),
            Some(ControllerDir::Named(name)) => {
                Some(normalize_view_path(&format!("{first}/{name}")))
            }
            Some(ControllerDir::Unknown) => None,
        }
    }
}

/// The view path a `withViewPath()` argument gives: an alias or an
/// absolute path, written out or built from `__DIR__`.
fn view_path_value(value: &Expression<'_>, dir: &Path) -> Option<String> {
    if let Expression::Literal(Literal::String(literal)) = value {
        let text = literal.value.and_then(crate::atom::literal_bytes_to_str)?;
        return (text.starts_with('@') || text.starts_with('/')).then(|| normalize_view_path(text));
    }
    let path = try_evaluate_path_expr(value, dir)?;
    path.starts_with('/').then(|| normalize_view_path(&path))
}

/// Finds the `with…()` calls that set up a class's renderer.
struct ViewPathWalker<'d> {
    dir: &'d Path,
}

impl<'ast, 'arena> Walker<'ast, 'arena, ViewPathCalls> for ViewPathWalker<'_> {
    fn walk_in_method_call(&self, node: &'ast MethodCall<'arena>, calls: &mut ViewPathCalls) {
        if let ClassLikeMemberSelector::Identifier(method) = &node.method {
            calls.record(bytes_to_str(method.value), &node.argument_list, self.dir);
        }
    }
}
