//! Byte-range and name helpers shared by the export passes.

use std::sync::Arc;

use crate::symbol_map::SelfStaticParentKind;
use crate::types::{ClassInfo, FunctionInfo};

use super::ByteRange;

pub(super) fn function_fqn(function: &FunctionInfo) -> String {
    function.namespace.as_ref().map_or_else(
        || function.name.to_string(),
        |namespace| format!("{namespace}\\{}", function.name),
    )
}

pub(super) fn self_reference_name(kind: SelfStaticParentKind) -> &'static str {
    match kind {
        SelfStaticParentKind::Self_ => "self",
        SelfStaticParentKind::Static => "static",
        SelfStaticParentKind::Parent => "parent",
        SelfStaticParentKind::This => "$this",
    }
}

pub(super) fn enclosing_class(classes: &[Arc<ClassInfo>], offset: u32) -> Option<&ClassInfo> {
    classes
        .iter()
        .filter(|class| class.decl_start_offset <= offset && offset <= class.end_offset)
        .min_by_key(|class| class.end_offset.saturating_sub(class.decl_start_offset))
        .map(AsRef::as_ref)
}

fn trim_range(source: &str, start: u32, end: u32) -> Option<ByteRange> {
    let mut start = usize::try_from(start).ok()?;
    let mut end = usize::try_from(end).ok()?;
    if start > end || end > source.len() {
        return None;
    }
    while start < end && source.as_bytes()[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && source.as_bytes()[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    Some(ByteRange {
        start: u32::try_from(start).unwrap_or(u32::MAX),
        end: u32::try_from(end).unwrap_or(u32::MAX),
    })
}

pub(super) fn argument_expression_range(
    source: &str,
    start: u32,
    end: u32,
    is_named: bool,
) -> Option<ByteRange> {
    let start = if is_named {
        let start_index = usize::try_from(start).ok()?;
        let end_index = usize::try_from(end).ok()?;
        let argument = source.get(start_index..end_index)?;
        let colon = argument.find(':')?;
        u32::try_from(start_index.saturating_add(colon).saturating_add(1)).ok()?
    } else {
        start
    };
    trim_range(source, start, end).filter(|range| range.start < range.end)
}

pub(super) fn token_range(
    source: &str,
    offset: u32,
    name: &str,
    includes_dollar: bool,
) -> ByteRange {
    let start = usize::try_from(offset).unwrap_or(0).min(source.len());
    let expected = if includes_dollar {
        format!("${name}")
    } else {
        name.to_string()
    };
    let token_start = if source[start..].starts_with(&expected) {
        start
    } else {
        source[start..]
            .find(&expected)
            .map_or(start, |relative| start.saturating_add(relative))
    };
    ByteRange {
        start: u32::try_from(token_start).unwrap_or(u32::MAX),
        end: u32::try_from(token_start.saturating_add(expected.len())).unwrap_or(u32::MAX),
    }
}

pub(super) fn find_token_between(source: &str, start: u32, end: u32, token: &str) -> u32 {
    let start = usize::try_from(start).unwrap_or(0).min(source.len());
    let end = usize::try_from(end)
        .unwrap_or(source.len())
        .min(source.len());
    source[start..end]
        .find(token)
        .and_then(|relative| u32::try_from(start.saturating_add(relative)).ok())
        .unwrap_or_else(|| u32::try_from(start).unwrap_or(u32::MAX))
}
