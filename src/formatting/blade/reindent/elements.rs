//! How the reindenter classifies HTML elements: the ones without a
//! closing tag, the ones whose children stay at their level, the ones
//! that flow with text, and the ones whose body is kept or shifted whole.

use super::RegionKind;

/// Elements that never have a closing tag.
pub(super) const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose children are not indented.
pub(super) const UNINDENTED_ELEMENTS: &[&str] = &["html"];

/// Elements that flow with text, so one followed by text on its line is
/// prose and opens nothing.
pub(super) const INLINE_ELEMENTS: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "dfn", "em", "i", "kbd", "mark", "q",
    "rp", "rt", "ruby", "s", "samp", "small", "span", "strong", "sub", "sup", "time", "u", "var",
];

/// Elements whose body is kept as it is: their whitespace is rendered
/// (`<pre>`, `<textarea>`).
pub(crate) const PRESERVED_ELEMENTS: &[&str] = &["pre", "textarea"];

/// Elements whose body is another language, shifted as a block rather
/// than reindented line by line.
pub(crate) const OPAQUE_ELEMENTS: &[&str] = &["script", "style"];

pub(super) fn element_region_kind(name: &str) -> Option<RegionKind> {
    if PRESERVED_ELEMENTS.contains(&name) {
        Some(RegionKind::Preserve)
    } else if OPAQUE_ELEMENTS.contains(&name) {
        Some(RegionKind::IndentPreserve)
    } else {
        None
    }
}

/// `<x-alert>`, `<flux:button>`, `<livewire:counter>`, and any custom
/// element: a name with a dash or a colon.
pub(crate) fn is_component_name(name: &str) -> bool {
    name.contains('-') || name.contains(':')
}
