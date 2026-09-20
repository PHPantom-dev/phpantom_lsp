//! Per-argument return types for the request input accessors.
//!
//! `header()`, `query()`, `post()`, `cookie()`, `input()` and `file()` all
//! declare one union covering every way they can be called, because a PHP
//! signature has no way to say that the answer depends on the arguments.
//! `$request->header('User-Agent', '')` is declared `string|array|null`
//! even though a header is never an array and the `''` rules out the null,
//! and `$request->query()` with no key is declared the same way even
//! though it can only be the whole array.
//!
//! Laravel decides all of it in `retrieveItem()` and `data_get()`:
//!
//! - No key at all returns the source bag's `all()`, which is an array.
//! - A key returns the item, or the default when it is missing — so a
//!   default that is not `null` is what rules the null branch out.
//! - A header is read through Symfony's `HeaderBag::get()`, which answers
//!   `?string`. The array branch belongs to the bag, not to one header.
//!
//! `file()` is the one accessor whose per-key answer is not fixed by the
//! source: a `photos[]` field holds a list of uploads and a `photo` field
//! holds one. The request's validation rules are what tell them apart
//! (`super::validated_shape` already turns a rules array into a shape),
//! and a key nothing describes is read as the single upload it almost
//! always is.

use std::sync::Arc;

use crate::Backend;
use crate::atom::atom;
use crate::php_type::{PhpType, TypeKind};
use crate::types::ClassInfo;

use super::validated_shape::rules_key_type;
use super::validation_rules::is_request_like;

/// FQN of the upload a `file()` call hands back.
///
/// Spelled without a leading backslash, which is how the class index and
/// every narrowing that compares against it name a class: an
/// `instanceof UploadedFile` guard cannot rule the list half of this type
/// out if the two spellings do not match.
const UPLOADED_FILE_FQN: &str = "Illuminate\\Http\\UploadedFile";

/// An input accessor whose return type depends on how it was called.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputAccessor {
    /// `header($key, $default)` — read through Symfony's `HeaderBag`.
    Header,
    /// `query()`, `post()`, `cookie()` — read through an input bag.
    Bag,
    /// `input($key, $default)` — `data_get()` over the merged input, so a
    /// keyed read can be anything the body carried.
    Input,
    /// `file($key, $default)`.
    File,
}

/// Classify a method name, or `None` when it is not an input accessor.
///
/// Runs on every method call the type engine resolves, so it compares in
/// place rather than lowercasing into a fresh `String`.
pub(crate) fn input_accessor(method: &str) -> Option<InputAccessor> {
    const ACCESSORS: [(&str, InputAccessor); 6] = [
        ("header", InputAccessor::Header),
        ("query", InputAccessor::Bag),
        ("post", InputAccessor::Bag),
        ("cookie", InputAccessor::Bag),
        ("input", InputAccessor::Input),
        ("file", InputAccessor::File),
    ];
    ACCESSORS
        .iter()
        .find(|(name, _)| method.eq_ignore_ascii_case(name))
        .map(|(_, accessor)| *accessor)
}

/// The arguments an input accessor was called with, as far as they are
/// readable at the call site.
pub(crate) struct AccessorArgs<'a> {
    /// Source text of the key argument, absent when the call passes none.
    pub key: Option<&'a str>,
    /// The type of the default argument, absent when the call passes none.
    ///
    /// Resolving it is what a default is for, so it is fetched lazily:
    /// only a keyed call with a second argument ever asks.
    pub default_type: &'a dyn Fn() -> Option<PhpType>,
}

/// The type an input accessor call returns, or `None` to leave the
/// declared union in place.
///
/// `offset` locates the call for the rules lookup a `file()` key needs;
/// everything else is decided from the arguments alone.
pub(crate) fn resolve_accessor_type(
    receiver: &ClassInfo,
    accessor: InputAccessor,
    args: &AccessorArgs<'_>,
    content: &str,
    offset: u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&Backend>,
) -> Option<PhpType> {
    if !is_request_like(receiver, class_loader) {
        return None;
    }

    // `header(null)` is the no-key form written out, and so is a call
    // that passes nothing.
    let key = args.key.filter(|text| !is_null_literal(text));
    let Some(key) = key else {
        return Some(whole_source_type(accessor));
    };

    let found = match accessor {
        // A header is a string; the array a `HeaderBag` holds per key is
        // not what `get()` hands back.
        InputAccessor::Header => PhpType::string(),
        InputAccessor::File => file_key_type(receiver, key, content, offset, class_loader, backend),
        // The body is whatever it is, which is what Laravel declares, so
        // only the missing-key branch is ours to narrow.
        InputAccessor::Bag | InputAccessor::Input => return None,
    };

    // The default is what the call gets when the key is missing, so it
    // replaces the null branch rather than joining it.
    let missing = match (args.default_type)() {
        Some(default) if !default.is_null() => default,
        _ => PhpType::null(),
    };
    Some(flat_union(found, missing))
}

/// Join two types into a union whose members sit side by side.
///
/// A nested union is not the same type to the narrowings that read one:
/// they walk the members a union names, and a member that is itself a
/// union names none of its own. A default of type `string|int` joined
/// whole would leave `header('X', $fallback)` as `string|(string|int)`,
/// which an `is_string()` guard can take nothing out of.
fn flat_union(left: PhpType, right: PhpType) -> PhpType {
    fn members(ty: PhpType) -> Vec<PhpType> {
        match ty.kind() {
            TypeKind::Union(parts) => parts.to_vec(),
            _ => vec![ty],
        }
    }
    let mut parts = members(left);
    parts.extend(members(right));
    PhpType::union(parts)
}

/// The type of the whole source bag, which is what a keyless call returns.
fn whole_source_type(accessor: InputAccessor) -> PhpType {
    let value = match accessor {
        // `HeaderBag::all()` holds every value a header was sent with.
        InputAccessor::Header => PhpType::parse("list<string|null>"),
        InputAccessor::File => {
            PhpType::parse(&format!("{UPLOADED_FILE_FQN}|list<{UPLOADED_FILE_FQN}>"))
        }
        InputAccessor::Bag | InputAccessor::Input => PhpType::mixed(),
    };
    PhpType::generic_array(PhpType::string(), value)
}

/// The upload type a `file()` key names.
///
/// A named field holds one upload; the list shape belongs to the `[]`
/// fields, and the request's validation rules are what say which is which
/// (`'photos' => 'array'` with `'photos.*' => 'file'`). A project that
/// takes several files under one key without validating it is the one
/// case this reads as a single upload.  That is the same trade the Laravel
/// PHPStan extensions make by declaring the union benevolent, except the
/// null a missing file produces stays checkable.
fn file_key_type(
    receiver: &ClassInfo,
    key: &str,
    content: &str,
    offset: u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    backend: Option<&Backend>,
) -> PhpType {
    let upload = PhpType::named(atom(UPLOADED_FILE_FQN));
    let Some(literal) = crate::util::unescape_php_string_literal(key.trim()) else {
        return upload;
    };
    let Some(backend) = backend else {
        return upload;
    };
    // The rules decide which of the two shapes the key holds; the type
    // itself is rebuilt here rather than passed through, so the class name
    // is spelled the one way every narrowing compares against.
    match rules_key_type(backend, receiver, &literal, content, offset, class_loader) {
        Some(from_rules) if holds_upload_list(&from_rules) => {
            PhpType::parse(&format!("list<{UPLOADED_FILE_FQN}>"))
        }
        _ => upload,
    }
}

/// Whether a type the rules produced is a collection of uploads rather
/// than one, which is what a `photos[]` field validates to.
fn holds_upload_list(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Generic(generic) => generic.args.iter().any(mentions_upload),
        TypeKind::Array(element) => mentions_upload(element),
        _ => false,
    }
}

/// Whether a type the rules produced describes an upload, at the top level
/// or as a collection's element.
fn mentions_upload(ty: &PhpType) -> bool {
    if holds_upload_list(ty) {
        return true;
    }
    ty.top_level_class_names()
        .iter()
        .any(|name| name.trim_start_matches('\\') == UPLOADED_FILE_FQN)
}

/// Whether an argument was written as the `null` literal.
fn is_null_literal(text: &str) -> bool {
    text.trim().eq_ignore_ascii_case("null")
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "request_input_tests.rs"]
mod tests;
