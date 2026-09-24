//! The attribute list of a component tag: lexing what a call site
//! passes, the slot names it declares, and the casing rules Blade applies
//! between an attribute name and the variable it becomes.

use super::*;

/// One attribute of a component tag, as its attribute list spells it.
///
/// Ranges index the text the list was lexed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TagAttribute {
    /// The name, without a bound attribute's leading `:` and without the
    /// `$` of the `:$name` shorthand. A `::` escape keeps the colon it
    /// protects (`::class` is named `:class`).
    pub(crate) name: Range<usize>,
    /// `:name="…"` or `:$name`: the value is a PHP expression, not text.
    pub(crate) bound: bool,
    /// The `:$name` shorthand, which passes the variable it names.
    pub(crate) shorthand: bool,
    /// The value text, between the quotes when quoted, or `None` for a
    /// bare attribute (`disabled`).
    pub(crate) value: Option<Range<usize>>,
    /// Whether the value was written between `"` or `'`.
    pub(crate) quoted: bool,
}

/// A component tag's attribute list, from just past the tag name to the
/// `>` or `/>` that ends the opening tag.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TagAttributes {
    pub(crate) attributes: Vec<TagAttribute>,
    /// The offset just past the `>`/`/>`, or the end of the text when the
    /// tag never closes.
    pub(crate) end: usize,
    /// Whether the opening tag ended with `/>`.
    pub(crate) self_closing: bool,
    /// Whether the tag's `>` was found at all. A quoted value that never
    /// closes swallows the rest of the text, so the tag is unclosed too.
    pub(crate) closed: bool,
}

/// Lex the attribute list of a component tag whose name ends at `start`.
///
/// This is the one reading of a tag's attributes both the preprocessor
/// (which turns them into the arguments of the call the tag makes) and
/// the call-site scan (which turns them into the component template's
/// variables) work from, so the two cannot disagree about what a tag
/// passes. An attribute value may hold a `>` (`:items="$a > $b"`), so the
/// tag ends at the first `>` outside a quoted value, not the first one.
///
/// The lexer is tolerant of malformed markup in one direction only: a
/// byte that starts no attribute is stepped over, so a broken tag cannot
/// spin the scan, but nothing is guessed at.
pub(crate) fn lex_tag_attributes(text: &str, start: usize) -> TagAttributes {
    let bytes = text.as_bytes();
    let mut lexed = TagAttributes::default();
    let mut i = start;

    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            None => break,
            Some(b'>') => {
                i += 1;
                lexed.closed = true;
                break;
            }
            Some(b'/') if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                lexed.closed = true;
                lexed.self_closing = true;
                break;
            }
            Some(b'/') => {
                i += 1;
                continue;
            }
            _ => {}
        }

        // `:$name` passes the variable it names.
        if bytes[i] == b':'
            && bytes.get(i + 1) == Some(&b'$')
            && bytes
                .get(i + 2)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            let name_start = i + 2;
            let mut j = name_start;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            lexed.attributes.push(TagAttribute {
                name: name_start..j,
                bound: true,
                shorthand: true,
                value: None,
                quoted: false,
            });
            i = j;
            continue;
        }

        // A single leading `:` marks a bound attribute; `::` is an
        // escaped literal colon (e.g. `::class`), and the attribute name
        // drops just the escape, not the real colon it protects.
        let bound = bytes[i] == b':' && bytes.get(i + 1) != Some(&b':');
        let name_start = if bytes[i] == b':' { i + 1 } else { i };
        let mut j = name_start;
        while j < bytes.len() && is_attr_name_char(bytes[j] as char) {
            j += 1;
        }
        if j == name_start {
            // Not an attribute token (e.g. a stray `<`); skip one byte so
            // a malformed tag cannot spin this loop forever.
            i += 1;
            continue;
        }
        let name = name_start..j;
        i = j;

        if bytes.get(i) != Some(&b'=') {
            lexed.attributes.push(TagAttribute {
                name,
                bound,
                shorthand: false,
                value: None,
                quoted: false,
            });
            continue;
        }
        i += 1;

        let quote = bytes.get(i).copied().filter(|b| *b == b'"' || *b == b'\'');
        let value_start = i + usize::from(quote.is_some());
        let mut k = value_start;
        match quote {
            Some(quote) => {
                while k < bytes.len() && bytes[k] != quote {
                    k += 1;
                }
            }
            None => {
                while k < bytes.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                    k += 1;
                }
            }
        }
        lexed.attributes.push(TagAttribute {
            name,
            bound,
            shorthand: false,
            value: Some(value_start..k),
            quoted: quote.is_some(),
        });
        // Past the closing quote, when there is one to be past.
        i = if quote.is_some() {
            (k + 1).min(bytes.len())
        } else {
            k
        };
    }

    lexed.end = i;
    lexed
}

/// Read the attribute list of a tag starting right after its name, up to
/// (and past) the tag's closing `>` or self-closing `/>`. Returns the
/// offset just past the close, plus the literal and bound attributes
/// found; `bound_index` is threaded through and bumped for every bound
/// attribute encountered, matching or not, to stay in sync with the
/// file-wide `blade_bound_attr_directive` call count.
pub(super) fn scan_tag_attributes(
    masked: &str,
    start: usize,
    consumed: &[String],
    bound_index: &mut usize,
) -> (usize, ComponentTagCall) {
    let lexed = lex_tag_attributes(masked, start);
    let mut call = ComponentTagCall::default();
    // A parameter can only be filled once, so a name an earlier attribute
    // of this tag already claimed is back to being ordinary markup — the
    // same rule the preprocessor applies on the emitting side.
    let mut unclaimed: Vec<&str> = consumed.iter().map(String::as_str).collect();
    let mut is_argument = |name: &str| match unclaimed.iter().position(|param| *param == name) {
        Some(index) => {
            unclaimed.remove(index);
            true
        }
        None => false,
    };

    for attr in &lexed.attributes {
        let written = &masked[attr.name.clone()];
        if attr.shorthand {
            // Bound, named after the variable. One the tag's own call
            // carries is not in the `blade_bound_attr_directive`
            // sequence at all.
            if !is_argument(written) {
                call.bound.push((written.to_string(), *bound_index));
                *bound_index += 1;
            }
            continue;
        }
        let name = camel_case_attr_name(written);
        let Some(value) = &attr.value else {
            // A bare attribute (`disabled`) is `true`. A bare *bound*
            // attribute (`:disabled`, no `=`) never reaches the
            // preprocessor's `blade_bound_attr_directive` emission (it
            // requires a quoted value), so there is nothing to correlate
            // for it.
            if !attr.bound {
                call.literal.push((name, PhpType::bool()));
            }
            continue;
        };
        if attr.bound {
            // An unquoted bound value is never recognised by the
            // preprocessor either; only a quoted one produced a
            // `blade_bound_attr_directive` call to correlate against.
            if attr.quoted && !is_argument(&name) {
                call.bound.push((name, *bound_index));
                *bound_index += 1;
            }
            continue;
        }
        let raw = &masked[value.clone()];
        let ty = if raw.contains("{{") || raw.contains("{!!") {
            // A literal attribute embedding a Blade echo is not a
            // constant string; fall back to a generic type rather
            // than reporting the raw `{{ $expr }}` text as the value.
            PhpType::string()
        } else {
            PhpType::literal_string_value(raw)
        };
        call.literal.push((name, ty));
    }

    (lexed.end, call)
}

/// The named slots (`<x-slot:title>` or the legacy `<x-slot
/// name="title">`) written as a direct child of one of `tag_names`'s
/// component tags, anywhere in `content`.
///
/// A slot's receiving component is its *nearest* enclosing `<x-…>` tag,
/// the same scoping Blade's own compiler applies: `ComponentTagCompiler`
/// lowers `<x-slot…>`/`</x-slot>` to `@slot(...)`/`@endslot` in a pass
/// that runs before component tags are compiled, and the runtime
/// `slot()`/`endSlot()` pair (`Illuminate\View\Concerns\ManagesComponents`)
/// files the slot under whichever component is innermost on the render
/// stack at that point. A plain HTML tag between a component and its
/// slot does not change that (Blade never tracks HTML nesting), but
/// another component tag in between does: its own slots are its own.
///
/// A name is returned once per distinct value, regardless of how many
/// occurrences across the file (or however many times a caller repeats
/// the same slot name) declare it: a component template cannot know at
/// preprocessing time which specific occurrence it is rendering for, so
/// every name any occurrence could pass has to be declared.
pub(crate) fn scan_component_tag_slots(content: &str, tag_names: &[String]) -> Vec<String> {
    if tag_names.is_empty() || !content.contains("<x-slot") {
        return Vec::new();
    }
    let masked = mask_inert_regions(content, true);
    let bytes = masked.as_bytes();
    // Currently open `<x-…>` component tags, nearest last. `<x-slot…>`
    // itself is never pushed here: it is not a component boundary, so a
    // slot cannot receive another slot.
    let mut stack: Vec<(&str, bool)> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'/') {
            if !masked[i + 2..].starts_with("x-") {
                i += 1;
                continue;
            }
            let name_start = i + 2 + "x-".len();
            let j = tag_name_end(bytes, name_start);
            let Some(close) = find_byte(bytes, j, b'>') else {
                break;
            };
            let name = &masked[name_start..j];
            if !is_slot_tag_name(name)
                && let Some(pos) = stack.iter().rposition(|(open, _)| *open == name)
            {
                stack.truncate(pos);
            }
            i = close + 1;
            continue;
        }
        if !bytes.get(i + 1).is_some_and(|b| b.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        if !masked[i + 1..].starts_with("x-") {
            i += 1;
            continue;
        }
        let name_start = i + 1 + "x-".len();
        let j = tag_name_end(bytes, name_start);
        let tag_name = &masked[name_start..j];
        let lexed = lex_tag_attributes(&masked, j);
        if is_slot_tag_name(tag_name) {
            if let Some((_, true)) = stack.last()
                && let Some(slot_name) = slot_tag_name(&masked, tag_name, &lexed)
                && !names.contains(&slot_name)
            {
                names.push(slot_name);
            }
        } else if lexed.closed && !lexed.self_closing {
            let is_match = tag_names.iter().any(|n| n == tag_name);
            stack.push((tag_name, is_match));
        }
        i = lexed.end;
    }
    names
}

/// Whether an `<x-…>` tag's bare name (after the `x-` prefix) opens a
/// slot: `slot` (the legacy `name="…"` form) or `slot:title` (the
/// inline form).
pub(super) fn is_slot_tag_name(name: &str) -> bool {
    name == "slot" || name.starts_with("slot:")
}

/// The name a `<x-slot…>` tag declares, or `None` when neither the
/// inline form nor a `name="…"` attribute names one (a `:name="$expr"`
/// bound name is dynamic and cannot be resolved here).
///
/// The inline form wins when both are written, matching
/// `ComponentTagCompiler::compileSlots`'s `$matches['inlineName'] ?:
/// $matches['name']`. It is also the only one ever camel-cased: Blade's
/// `Str::camel` runs on the inline name when it contains a hyphen (a
/// PHP variable cannot spell one), but an attribute-form name is used
/// verbatim, hyphens and all, so a hyphenated one is written down as a
/// slot key `extract()` can never bind to a variable — the same fate an
/// unbindable component-tag attribute name has (the preprocessor's
/// `is_php_variable_name` skips declaring either).
fn slot_tag_name(masked: &str, tag_name: &str, lexed: &TagAttributes) -> Option<String> {
    if let Some(inline) = tag_name.strip_prefix("slot:") {
        return Some(if inline.contains('-') {
            camel_case_attr_name(inline)
        } else {
            inline.to_string()
        });
    }
    lexed.attributes.iter().find_map(|attr| {
        (!attr.bound && &masked[attr.name.clone()] == "name")
            .then(|| attr.value.clone())
            .flatten()
            .map(|value| masked[value].to_string())
    })
}

/// The name a `<x-slot>` tag's opening tag gives it through its
/// `name="…"` attribute, lexing from `name_end`, the end of the tag name.
pub(crate) fn legacy_slot_name(content: &str, name_end: usize) -> Option<String> {
    slot_tag_name(content, "slot", &lex_tag_attributes(content, name_end))
}

/// Convert a kebab-case attribute name to the camelCase variable name
/// Blade exposes it as (`Illuminate\Support\Str::camel`). A PHP variable
/// name cannot contain a hyphen, so only the camelCase form of a
/// hyphenated attribute is ever accessible inside the template.
pub(crate) fn camel_case_attr_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '-' || ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Convert a camelCase name to the kebab-case attribute (or tag-name)
/// segment it is written as, matching `Illuminate\Support\Str::kebab`: a
/// delimiter goes before every capital that isn't the first character, and
/// existing separators are kept.
///
/// The inverse of [`camel_case_attr_name`] for the names Blade round-trips
/// (`hairAnalysis` ↔ `hair-analysis`), and the transform that turns a
/// component class's name into the tag that reaches it.
pub(crate) fn kebab_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.char_indices() {
        if ch.is_uppercase() && i > 0 {
            out.push('-');
        }
        out.extend(ch.to_lowercase());
    }
    out
}
