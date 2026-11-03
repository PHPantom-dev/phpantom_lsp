//! Connection names whose config subtree is determined by the receiver's type.

use super::*;
use crate::virtual_members::laravel::helpers::resolved_string_literal;

/// The receiver and parameter that identify a Laravel connection selection.
pub(crate) fn connection_receiver_method(method: &str) -> Option<(ReceiverClass, &'static str)> {
    if method.eq_ignore_ascii_case("connection") {
        Some((ReceiverClass::LaravelConnection, "name"))
    } else if method.eq_ignore_ascii_case("onConnection") {
        Some((ReceiverClass::LaravelQueueConnection, "connection"))
    } else {
        None
    }
}

/// Record a literal for the shared type engine to classify after indexing.
pub(in crate::symbol_map::extraction) fn record_connection_site(
    receiver: ReceiverClass,
    value: &Expression<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    let Some((key, start, end)) = resolved_string_literal(value, ctx.content) else {
        return;
    };
    ctx.typed_receiver_sites.push(TypedReceiverSite {
        start: start as u32,
        end: end as u32,
        key: key.to_string(),
        receiver,
        is_write: false,
        is_optional: false,
        guards: 0,
    });
}

/// Record a connection property default without assuming what its class is.
pub(in crate::symbol_map::extraction) fn record_connection_property(
    property: &Property<'_>,
    ctx: &mut ExtractionCtx<'_>,
) {
    if property.modifiers().iter().any(Modifier::is_static) {
        return;
    }
    let items = match property {
        Property::Plain(plain) => plain.items.as_slice(),
        Property::Hooked(hooked) => std::slice::from_ref(&hooked.item),
    };
    for item in items {
        if let PropertyItem::Concrete(item) = item
            && item.variable.name == b"$connection"
        {
            record_connection_site(ReceiverClass::LaravelConnectionProperty, item.value, ctx);
        }
    }
}

/// The object whose connection a plain property assignment selects.
pub(crate) fn connection_assignment_receiver<'ast, 'arena>(
    assign: &'ast Assignment<'arena>,
) -> Option<&'ast Expression<'arena>> {
    if !assign.operator.is_assign() {
        return None;
    }
    let Expression::Access(Access::Property(access)) = assign.lhs else {
        return None;
    };
    let ClassLikeMemberSelector::Identifier(property) = &access.property else {
        return None;
    };
    (property.value == b"connection").then_some(access.object)
}
