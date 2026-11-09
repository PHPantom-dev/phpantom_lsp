//! The message IDs a Yii translator's `translate()` names (see
//! [`crate::yii::translations`]).
//!
//! `translate()` is far too common a name to claim on sight, so a call is
//! recorded as a [`TypedReceiverSite`] for the type engine to confirm.  The
//! category the ID is looked up in is read where the call is written: its
//! `category` argument, or else the one a `withDefaultCategory()` gives the
//! translator, in the receiver chain or anywhere in the class.

use mago_span::HasSpan;
use mago_syntax::walker::Walker;

use super::*;
use crate::symbol_map::ReceiverClass;
use crate::virtual_members::laravel::helpers::resolved_string_literal;
use crate::yii::translations::{is_message_id, message_key};

/// The category the translators of the code being extracted look IDs up in
/// when a call names none.
#[derive(Debug, Clone, Default)]
pub(in crate::symbol_map::extraction) enum YiiCategory {
    /// The default the DI configuration gives the translator.
    #[default]
    Default,
    /// The one every `withDefaultCategory()` gives it.
    Named(String),
    /// One that cannot be told: a category that is not a literal, or
    /// several different ones.
    Unknown,
}

impl YiiCategory {
    /// Add the category one `withDefaultCategory()` gives, `None` for one
    /// that is not a literal.
    fn add(&mut self, category: Option<&str>) {
        *self = match (std::mem::take(self), category) {
            (Self::Default, Some(name)) => Self::Named(name.to_string()),
            (Self::Named(known), Some(name)) if known == name => Self::Named(known),
            _ => Self::Unknown,
        };
    }
}

/// The category a `withDefaultCategory()` call's arguments give, `None`
/// when it is not a literal.
fn given_category<'a>(argument_list: &ArgumentList<'a>, content: &str) -> Option<&'a str> {
    let argument = argument_list.arguments.first()?;
    resolved_string_literal(argument.value(), content).map(|(name, _, _)| name)
}

/// The category the `withDefaultCategory()` calls in `members` give the
/// class's translators.
pub(in crate::symbol_map::extraction) fn yii_class_category<'a>(
    span: mago_span::Span,
    members: impl Iterator<Item = &'a ClassLikeMember<'a>>,
    ctx: &ExtractionCtx<'_>,
) -> YiiCategory {
    if !ctx.yii {
        return YiiCategory::Default;
    }
    let text = ctx
        .content
        .as_bytes()
        .get(span.start.offset as usize..span.end.offset as usize)
        .unwrap_or_default();
    if memchr::memmem::find(text, b"withDefaultCategory").is_none() {
        return YiiCategory::Default;
    }
    let mut finder = CategoryFinder {
        content: ctx.content,
        category: YiiCategory::Default,
        chained: Vec::new(),
    };
    for member in members {
        CategoryWalker.walk_class_like_member(member, &mut finder);
    }
    finder.category
}

struct CategoryFinder<'c> {
    content: &'c str,
    category: YiiCategory,
    /// Where the `withDefaultCategory()` calls another call is made on
    /// start, which give the category to that call alone (see
    /// [`chain_category`]).
    chained: Vec<u32>,
}

struct CategoryWalker;

impl<'ast, 'arena> Walker<'ast, 'arena, CategoryFinder<'_>> for CategoryWalker {
    fn walk_in_method_call(&self, node: &'ast MethodCall<'arena>, finder: &mut CategoryFinder<'_>) {
        // A call is entered before the receiver it is made on.
        if let Expression::Call(Call::Method(receiver)) = node.object {
            finder.chained.push(receiver.span().start.offset);
        }
        if let ClassLikeMemberSelector::Identifier(name) = &node.method
            && name.value.eq_ignore_ascii_case(b"withDefaultCategory")
            && !finder.chained.contains(&node.span().start.offset)
        {
            let category = given_category(&node.argument_list, finder.content);
            finder.category.add(category);
        }
    }
}

/// The category a `withDefaultCategory()` in the receiver chain gives,
/// `$translator->withDefaultCategory('blog')->translate('title')`, or
/// `None` when the chain gives none.  The call nearest the `translate()`
/// is the one that counts.
fn chain_category(object: &Expression<'_>, content: &str) -> Option<YiiCategory> {
    let mut current = object;
    while let Expression::Call(Call::Method(call)) = current {
        if let ClassLikeMemberSelector::Identifier(name) = &call.method
            && name.value.eq_ignore_ascii_case(b"withDefaultCategory")
        {
            return Some(
                given_category(&call.argument_list, content).map_or(YiiCategory::Unknown, |name| {
                    YiiCategory::Named(name.to_string())
                }),
            );
        }
        current = call.object;
    }
    None
}

/// Record the message ID a `translate()` call names, in the category it is
/// looked up in, for the type engine to confirm that its receiver is a
/// translator.  A call whose category cannot be told records none.
pub(in crate::symbol_map::extraction) fn record_yii_translator_sites(
    object: &Expression<'_>,
    argument_list: &ArgumentList<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let mut id = None;
    // `None` until the call names a category, then whether it is a literal.
    let mut category: Option<Option<&str>> = None;
    for (index, argument) in argument_list.arguments.iter().enumerate() {
        let (parameter, value) = match argument {
            Argument::Named(named) => (crate::atom::bytes_to_str(named.name.value), named.value),
            Argument::Positional(positional) => match index {
                0 => ("id", positional.value),
                2 => ("category", positional.value),
                _ => continue,
            },
        };
        match parameter {
            "id" => id = Some(value),
            "category" if !matches!(value, Expression::Literal(Literal::Null(_))) => {
                category =
                    Some(resolved_string_literal(value, ctx.content).map(|(name, _, _)| name));
            }
            _ => {}
        }
    }
    let Some((id, start, end)) = id.and_then(|id| resolved_string_literal(id, ctx.content)) else {
        return;
    };
    if !is_message_id(id) {
        return;
    }
    let scope = match category {
        Some(Some(name)) => YiiCategory::Named(name.to_string()),
        Some(None) => YiiCategory::Unknown,
        None => chain_category(object, ctx.content).unwrap_or_else(|| ctx.yii_category.clone()),
    };
    let key = match &scope {
        YiiCategory::Default => message_key(None, id),
        YiiCategory::Named(name) => message_key(Some(name), id),
        YiiCategory::Unknown => return,
    };
    ctx.typed_receiver_sites.push(TypedReceiverSite {
        start: start as u32,
        end: end as u32,
        key,
        is_write: false,
        is_optional: false,
        guards: 0,
        receiver: ReceiverClass::YiiTranslator,
    });
}
