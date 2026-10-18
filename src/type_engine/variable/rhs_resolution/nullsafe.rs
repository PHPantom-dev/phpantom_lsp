//! The `?->` short-circuit: whether a member access's receiver can be
//! `null`, and what a `null` there does to the rest of the chain.

use mago_syntax::cst::*;

use crate::php_type::{PhpType, TypeKind};
use crate::types::ResolvedType;

use super::peel_type_transparent;

/// Whether a member access's receiver can be `null`, as far as a
/// short-circuiting `?->` is concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiverNullability {
    Never,
    Maybe,
    Always,
}

impl ReceiverNullability {
    /// Read the nullability off a receiver's resolved types.  `mixed`
    /// counts as non-null: whatever a member of it reads is `mixed`
    /// already, so the extra `null` would say nothing.
    pub(super) fn of(resolved: &[ResolvedType]) -> Self {
        let mut any_null = false;
        let mut all_null = !resolved.is_empty();
        for rt in resolved {
            let ty = &rt.type_string;
            if ty.is_null() {
                any_null = true;
            } else {
                all_null = false;
                any_null |= matches!(ty.kind(), TypeKind::Nullable(_))
                    || matches!(ty.kind(), TypeKind::Union(members) if members.iter().any(PhpType::is_null));
            }
        }
        match (all_null, any_null) {
            (true, _) => Self::Always,
            (false, true) => Self::Maybe,
            (false, false) => Self::Never,
        }
    }
}

/// Whether a member access chain runs through a `?->` before reaching
/// `expr`'s own link, so that a `null` there short-circuits the rest of
/// the chain rather than being read through.
fn chain_has_nullsafe<'b>(mut expr: &'b Expression<'b>) -> bool {
    loop {
        expr = peel_type_transparent(expr);
        expr = match expr {
            Expression::Access(Access::NullSafeProperty(_))
            | Expression::Call(Call::NullSafeMethod(_)) => return true,
            Expression::Access(Access::Property(pa)) => pa.object,
            Expression::Call(Call::Method(mc)) => mc.object,
            Expression::ArrayAccess(aa) => aa.array,
            _ => return false,
        };
    }
}

/// Fold a `?->` short-circuit into a member access's result.
///
/// A nullsafe link whose receiver is `null` evaluates to `null` without
/// reading the member, and so does every link after it in the same chain.
/// `nullsafe` is whether this link is `?->` itself; `object` is its
/// receiver expression, consulted only when the receiver can be `null`.
pub(super) fn apply_nullsafe_short_circuit(
    result: &mut Vec<ResolvedType>,
    receiver: ReceiverNullability,
    nullsafe: bool,
    object: &Expression<'_>,
) {
    if receiver == ReceiverNullability::Never || !(nullsafe || chain_has_nullsafe(object)) {
        return;
    }
    if receiver == ReceiverNullability::Always {
        *result = vec![ResolvedType::from_type_string(PhpType::null())];
        return;
    }
    if result.is_empty() || result.iter().any(|rt| rt.type_string.accepts_null()) {
        return;
    }
    // A lone entry carries the `null` in its own type, the way a `?Foo`
    // return does, so narrowing that rules the class in rules it out.
    if let [only] = result.as_mut_slice() {
        only.type_string = std::mem::replace(&mut only.type_string, PhpType::null()).or_null();
        return;
    }
    result.push(ResolvedType::from_type_string(PhpType::null()));
}
