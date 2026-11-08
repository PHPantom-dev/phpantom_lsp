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
use crate::virtual_members::laravel::helpers::{extract_string_literal, resolved_string_literal};
use crate::yii::views::{
    controller_name, is_absolute_view_path, is_layout_method, is_view_render_method, layout_key,
    normalize_view_path, path_value, view_key,
};

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

/// Record the files an asset bundle's `$css` and `$js` list, each keyed by
/// the directory the bundle names and the entry (see
/// [`crate::yii::assets`]).
pub(in crate::symbol_map::extraction) fn emit_yii_asset_bundle_spans(
    class: &Class<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(file_dir) = ctx.yii_dir else {
        return;
    };
    let is_bundle = crate::yii::assets::is_asset_bundle(class, |parent| {
        ctx.resolved_name_at(parent.span().start.offset)
            .unwrap_or_else(|| bytes_to_str(parent.value()))
            .to_string()
    });
    if !is_bundle {
        return;
    }
    let Some(dir) = crate::yii::assets::bundle_dir(class, file_dir) else {
        return;
    };
    for entry in crate::yii::assets::bundle_entries(class) {
        let Some((text, start, end)) = resolved_string_literal(entry, ctx.content) else {
            continue;
        };
        let Some(key) = crate::yii::assets::asset_file_key(&dir, text) else {
            continue;
        };
        ctx.spans.push(SymbolSpan {
            start: start as u32,
            end: end as u32,
            kind: SymbolKind::StringKey {
                key,
                kind: StringKeyKind::YiiAssetFile,
                is_write: false,
                is_optional: false,
            },
        });
    }
}

/// Record the alias a string starts with, `@runtime` in `'@runtime/logs'`,
/// as a span over the alias alone.  Any string can name a path, so every
/// string of a Yii project is read.
pub(in crate::symbol_map::extraction) fn record_yii_alias(
    literal: &LiteralString<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let start = literal.span.start.offset as usize + 1;
    let end = (literal.span.end.offset as usize).saturating_sub(1);
    let Some(text) = ctx.content.get(start..end) else {
        return;
    };
    let Some(alias) = crate::yii::aliases::alias_prefix(text) else {
        return;
    };
    ctx.spans.push(SymbolSpan {
        start: start as u32,
        end: (start + alias.len()) as u32,
        kind: SymbolKind::StringKey {
            key: alias.to_string(),
            kind: StringKeyKind::YiiAlias,
            is_write: false,
            is_optional: false,
        },
    });
}

/// Record the key a read of the Yii `$params` names last,
/// `'name'` in `$params['application']['name']`, keyed by the whole path
/// of keys the read names.  Each access of a chain records its own key, so
/// `$params['application']` is recorded by the access inside this one.
pub(in crate::symbol_map::extraction) fn record_yii_param_read(
    access: &ArrayAccess<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Expression::Literal(Literal::String(last)) = access.index else {
        return;
    };
    let Some(last_key) = last.value.and_then(crate::atom::literal_bytes_to_str) else {
        return;
    };
    let mut keys = vec![last_key];
    let mut array = access.array;
    loop {
        match array {
            Expression::ArrayAccess(inner) => {
                let Expression::Literal(Literal::String(key)) = inner.index else {
                    return;
                };
                let Some(key) = key.value.and_then(crate::atom::literal_bytes_to_str) else {
                    return;
                };
                keys.push(key);
                array = inner.array;
            }
            Expression::Variable(Variable::Direct(variable)) if variable.name == b"$params" => {
                break;
            }
            _ => return,
        }
    }
    keys.reverse();
    ctx.spans.push(SymbolSpan {
        start: last.span.start.offset + 1,
        end: last.span.end.offset.saturating_sub(1),
        kind: SymbolKind::StringKey {
            key: crate::yii::params::param_key(&keys),
            kind: StringKeyKind::YiiParam,
            is_write: false,
            is_optional: false,
        },
    });
}

/// Mark the `$params` reads recorded from `from` on as ones the code is
/// written to do without, as the reads an `isset()` or the left side of a
/// `??` makes are.
pub(in crate::symbol_map::extraction) fn mark_yii_param_reads_optional(
    from: usize,
    ctx: &mut ExtractionCtx<'_>,
) {
    for span in ctx.spans.iter_mut().skip(from) {
        if let SymbolKind::StringKey {
            kind: StringKeyKind::YiiParam,
            is_optional,
            ..
        } = &mut span.kind
        {
            *is_optional = true;
        }
    }
}

/// Record the view the first argument of a render call or of a
/// `withLayout()` names, and the directory a `withViewPath()` names.
pub(in crate::symbol_map::extraction) fn record_yii_view_sites(
    object: &Expression<'_>,
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(dir) = ctx.yii_dir else {
        return;
    };
    if member_name.eq_ignore_ascii_case("withViewPath") {
        record_yii_view_path_site(argument_list, dir, ctx);
        return;
    }
    let is_layout = is_layout_method(member_name);
    if !is_layout && !is_view_render_method(member_name) {
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
        if !is_absolute_view_path(&path) {
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
        // A template has no renderer whose view path a layout could be
        // relative to.
        YiiViewScope::Template if is_layout => layout_key(name, None),
        YiiViewScope::Template => {
            let dir = dir.to_string_lossy();
            view_key(name, Some(&dir), Some(&dir))
        }
        YiiViewScope::Class(base) => {
            let class_fqn = ctx
                .class_name_offset
                .and_then(|offset| ctx.resolved_name_at(offset));
            let chain_base = chain_view_base(object, dir, class_fqn);
            let base = chain_base.as_ref().unwrap_or(base).as_deref();
            if is_layout {
                layout_key(name, base)
            } else {
                view_key(name, base, None)
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
        guards: 0,
        receiver: ReceiverClass::YiiViewRenderer,
    });
}

/// Record each class name the string arguments of a `class()` or an
/// `addClass()` name, `'card'` and `'card-body'` in
/// `->class('card card-body')`, for the type engine to confirm the receiver
/// is an HTML tag.
pub(in crate::symbol_map::extraction) fn record_yii_css_class_sites(
    member_name: &str,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if !ctx.yii
        || !(member_name.eq_ignore_ascii_case("class")
            || member_name.eq_ignore_ascii_case("addClass"))
    {
        return;
    }
    for argument in argument_list.arguments.iter() {
        let Argument::Positional(argument) = argument else {
            continue;
        };
        let Some((text, start, _)) = extract_string_literal(argument.value, ctx.content) else {
            continue;
        };
        for (offset, name) in crate::yii::css_class_names(text) {
            let start = (start + offset) as u32;
            ctx.typed_receiver_sites.push(TypedReceiverSite {
                start,
                end: start + name.len() as u32,
                key: name.to_string(),
                is_write: false,
                is_optional: false,
                guards: 0,
                receiver: ReceiverClass::HtmlTag,
            });
        }
    }
}

/// Record the directory the argument of a `withViewPath()` names, for the
/// type engine to confirm the receiver is a view renderer.
fn record_yii_view_path_site(
    argument_list: &ArgumentList<'_>,
    dir: &Path,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some(Argument::Positional(argument)) = argument_list.arguments.first() else {
        return;
    };
    let Some(key) = path_value(argument.value, dir) else {
        return;
    };
    let literal = match argument.value {
        Expression::Binary(binary) if binary.operator.is_concatenation() => binary.rhs,
        value => value,
    };
    let Some((_, start, end)) = resolved_string_literal(literal, ctx.content) else {
        return;
    };
    ctx.typed_receiver_sites.push(TypedReceiverSite {
        start: start as u32,
        end: end as u32,
        key,
        is_write: false,
        is_optional: false,
        guards: 0,
        receiver: ReceiverClass::YiiViewPath,
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
                .push(first.and_then(|value| path_value(value, dir)));
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
