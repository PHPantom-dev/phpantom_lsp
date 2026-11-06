//! What `->__invoke()` and `->call()` return on a closure signature.
//!
//! A value typed `Closure(int): User` resolves its members on the `Closure`
//! class, whose `__invoke()` and `call()` are declared `mixed`. The
//! signature on the receiver's type says what calling it returns, so both
//! call-resolution entry points read it from there instead.

use crate::php_type::PhpType;
use crate::types::ResolvedType;

/// Whether `method_name` runs the closure it is called on.
///
/// Checked before looking at the receiver, so an unrelated method call
/// costs one string comparison.
pub(crate) fn is_closure_invoke_call(method_name: &str) -> bool {
    method_name.eq_ignore_ascii_case("__invoke") || method_name.eq_ignore_ascii_case("call")
}

/// The return type of the closure signatures `receiver` is typed with, or
/// `None` when some alternative is not a closure signature.
pub(crate) fn closure_invoke_return(receiver: &[ResolvedType]) -> Option<PhpType> {
    let mut returns = Vec::with_capacity(receiver.len());
    for rt in receiver {
        returns.push(rt.type_string.closure_signature_return()?);
    }
    match returns.len() {
        0 => None,
        1 => returns.pop(),
        _ => Some(PhpType::join_runtime_value_types(returns)),
    }
}
