//! `mb_convert_encoding()`: the subject's own shape with its strings
//! converted, and `false` only when the source encoding has to be detected
//! among several. Mirrors PHPStan's
//! `MbConvertEncodingFunctionReturnTypeExtension`.

use crate::php_type::{PhpType, ShapeEntry, TypeKind};

use super::{CallArgs, literal_strings};

pub(super) fn return_type(args: &CallArgs<'_>) -> Option<PhpType> {
    let subject = args.resolve(&args.text("$string")?)?;
    let mut results: Vec<PhpType> = Vec::new();
    for member in subject.strip_benevolence().union_members() {
        let ty = if member.is_mixed() {
            PhpType::union(vec![PhpType::array(), PhpType::string()])
        } else if member.is_array_like() {
            converted_array(member)?
        } else if member.is_string_subtype()
            || member.is_int_subtype()
            || member.is_float_subtype()
            || member.is_bool()
            || member.is_true()
            || member.is_false()
        {
            // A scalar is coerced to the string parameter.
            PhpType::string()
        } else {
            return None;
        };
        for ty in ty.union_members() {
            if !results.contains(ty) {
                results.push(ty.clone());
            }
        }
    }
    if may_fail(args) {
        results.push(PhpType::false_());
    }
    Some(PhpType::union(results))
}

/// Whether the call can return `false`: only when the source encoding is
/// detected among several (a comma-separated list, `auto`, or an array of
/// more than one), which can fail to find one. A single source encoding,
/// or the internal one when none is given, always converts.
fn may_fail(args: &CallArgs<'_>) -> bool {
    let Some(text) = args.text("$from_encoding") else {
        return false;
    };
    if text.trim().eq_ignore_ascii_case("null") {
        return false;
    }
    let Some(from) = args.resolve(&text) else {
        return true;
    };
    from.union_members().into_iter().any(|member| {
        if let Some(names) = literal_strings(member) {
            return names
                .iter()
                .any(|name| name.contains(',') || name.trim().eq_ignore_ascii_case("auto"));
        }
        match member.shape_parts() {
            Some(parts) if parts.tail.is_none() => parts.entries.len() > 1,
            _ => !member.is_null(),
        }
    })
}

/// An array subject with every string in it converted, keeping its keys and
/// its other values.
fn converted_array(array: &PhpType) -> Option<PhpType> {
    let value = |ty: &PhpType| -> Option<PhpType> {
        if ty.is_string_subtype() {
            Some(PhpType::string())
        } else if ty.is_array_like() {
            converted_array(ty)
        } else {
            Some(ty.clone())
        }
    };
    match array.kind() {
        TypeKind::ArrayShape(entries) => Some(PhpType::array_shape(
            entries
                .iter()
                .map(|entry| {
                    Some(ShapeEntry {
                        key: entry.key.clone(),
                        optional: entry.optional,
                        value_type: value(&entry.value_type)?,
                    })
                })
                .collect::<Option<_>>()?,
        )),
        TypeKind::Generic(generic) => {
            let mut args = generic.args.clone();
            let last = args.last_mut()?;
            *last = value(last)?;
            Some(PhpType::generic_atom(generic.name, args))
        }
        TypeKind::Array(inner) => Some(PhpType::array_of(value(inner)?)),
        _ => Some(array.clone()),
    }
}
