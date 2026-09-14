use super::component_call::{
    OpenComponentCall, bound_attr_open_len, bound_attr_spans_lines, component_tag_at,
};
use super::shared::{LineOut, Lowering, flush_buffer, utf16_count};
use super::{ComponentResolver, Mode};
use crate::blade::component_tags::camel_case_attr_name;

/// Where the HTML scanner is relative to tag markup: between the `<` and
/// `>` of a tag, and (when inside a tag) inside a quoted attribute value.
/// Both persist across lines so multi-line tags are tracked correctly.
/// They gate recognition of `:name="$expr"` bound attributes, which are
/// only valid at attribute position inside a tag.
pub(super) struct HtmlPos {
    pub(super) in_tag: bool,
    pub(super) attr_string: Option<char>,
}

/// The bound attribute currently open in [`Mode::BoundAttr`].
pub(super) struct BoundAttr {
    /// What closes the expression: `);` for the
    /// `blade_bound_attr_directive(` call an ordinary bound attribute
    /// becomes, and `;` for one that is an argument of the surrounding
    /// tag's component call and is bound to a variable for it.
    pub(super) suffix: &'static str,
    /// Whether the closing quote is on a later line, so the expression
    /// must stay open at end of line instead of being closed off. Set
    /// when the attribute opens; see `bound_attr_spans_lines`.
    pub(super) multiline: bool,
}

/// A Blade component tag opening at the cursor.
pub(super) fn open(
    remaining: &[char],
    html: &mut HtmlPos,
    following: &[&str],
    components: Option<&dyn ComponentResolver>,
    open_call: &mut Option<OpenComponentCall>,
) -> Option<Lowering> {
    let match_len;
    let replacement;
    let next_mode = Mode::Html;

    if html.attr_string.is_none()
        && let Some(tag) = component_tag_at(remaining)
    {
        // A Blade component tag. Only the tag *name* is
        // consumed here: the attribute list keeps flowing
        // through the HTML scanner, so a bound attribute's
        // expression stays where the template wrote it and the
        // markup around it still becomes what it always did.
        match_len = tag.len;
        // A tag opening inside another tag is malformed markup;
        // leaving the outer call to be closed by the first `>`
        // keeps the emitted PHP balanced.
        let target = open_call
            .is_none()
            .then(|| tag.resolve(components))
            .flatten();
        let (text, call) = tag.emit(target, &remaining[tag.len..], following);
        replacement = text;
        if call.is_some() {
            *open_call = call;
        }
        // The tag's `<` went into the replacement instead of
        // reaching the tag-state tracker below, so mark the
        // tag open by hand — otherwise `:attr="$expr"` inside
        // a component tag would not be at attribute position.
        html.in_tag = true;
    } else {
        return None;
    }

    Some(Lowering {
        match_len,
        replacement,
        next_mode,
    })
}

/// The `>` or `/>` that closes a component tag.
pub(super) fn close(
    remaining: &[char],
    html: &mut HtmlPos,
    open_call: &mut Option<OpenComponentCall>,
) -> Option<Lowering> {
    let match_len;
    let replacement;
    let next_mode = Mode::Html;

    if open_call.is_some()
        && html.attr_string.is_none()
        && (remaining.starts_with(&['>']) || remaining.starts_with(&['/', '>']))
    {
        // The tag closes, which is where the call it makes is
        // emitted: everything between the tag's name and here
        // is markup that became statements.
        match_len = if remaining[0] == '/' { 2 } else { 1 };
        replacement = open_call.take().expect("call is open").close();
        html.in_tag = false;
    } else {
        return None;
    }

    Some(Lowering {
        match_len,
        replacement,
        next_mode,
    })
}

/// A bound attribute opening at the cursor.
pub(super) fn bound_attr(
    remaining: &[char],
    line_chars: &[char],
    char_idx: usize,
    html: &HtmlPos,
    following: &[&str],
    open_call: &mut Option<OpenComponentCall>,
    bound_attr: &mut BoundAttr,
) -> Option<Lowering> {
    let mut match_len = 0;
    let mut replacement = String::new();
    let mut next_mode = Mode::Html;

    if remaining.starts_with(&[':'])
        && html.in_tag
        && html.attr_string.is_none()
        && (char_idx == 0 || line_chars[char_idx - 1].is_ascii_whitespace())
        && remaining.get(1) != Some(&':')
    {
        // A Blade component bound attribute at attribute
        // position: `:name="$expr"`, `:name='$expr'`, or the
        // `:$var` shorthand. The expression stays where the
        // template wrote it, either as an argument of the
        // component call the tag opened or, when it names no
        // parameter of it, as a `blade_bound_attr_directive(...)`
        // call of its own so its variables are still seen. That
        // marker is exclusive to bound attributes (unlike the
        // generic `blade_directive` shared by `@class`, `@json`,
        // and friends), so a scan counting bound attributes can
        // count its calls without another directive's call
        // shifting the sequence. The rest of the tag stays
        // masked. A leading `::` is an escaped literal colon and
        // is left alone.
        let shorthand = remaining.get(1) == Some(&'$')
            && remaining
                .get(2)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_');
        // `:$name` names the variable it passes; `:name="…"`
        // has its name between the `:` and the `="`.
        let name_span = if shorthand {
            Some(
                2..2 + remaining[2..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
                    .count(),
            )
        } else {
            bound_attr_open_len(remaining).map(|open_len| 1..open_len - 2)
        };

        if let Some(name_span) = name_span {
            let name = camel_case_attr_name(&remaining[name_span].iter().collect::<String>());
            let argument = open_call
                .as_mut()
                .and_then(|call| call.take(&name))
                .map(|variable| format!(" {variable} = "));
            let (prefix, suffix) = match &argument {
                Some(prefix) => (prefix.as_str(), ";"),
                None => (" blade_bound_attr_directive(", ");"),
            };
            replacement = prefix.to_string();
            bound_attr.suffix = suffix;

            if shorthand {
                match_len = 1;
                next_mode = Mode::BoundAttr(None);
                bound_attr.multiline = false;
            } else {
                let open_len = bound_attr_open_len(remaining).expect("name parsed");
                let quote = remaining[open_len - 1];
                match_len = open_len;
                next_mode = Mode::BoundAttr(Some(quote));
                bound_attr.multiline =
                    bound_attr_spans_lines(quote, &remaining[open_len..], following);
            }
        }
    } else {
        return None;
    }

    Some(Lowering {
        match_len,
        replacement,
        next_mode,
    })
}

/// Close a bound-attribute expression at its terminator, reporting whether
/// the terminator was reached.
pub(super) fn close_bound_attr(
    term: Option<char>,
    ch: char,
    in_string: Option<char>,
    bound_attr: &BoundAttr,
    out: LineOut<'_>,
) -> bool {
    let at_end = match term {
        Some(delim) => in_string.is_none() && ch == delim,
        None => in_string.is_none() && !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'),
    };
    if !at_end {
        return false;
    }
    flush_buffer(
        out.processed,
        out.buffer,
        Mode::BoundAttr(term),
        *out.current_utf16_col,
        out.adjustments,
    );
    let start_suffix = utf16_count(out.processed) as u32;
    out.processed.push_str(bound_attr.suffix);
    let end_suffix = utf16_count(out.processed) as u32;
    out.adjustments.push((*out.current_utf16_col, start_suffix));
    out.adjustments.push((*out.current_utf16_col, end_suffix));
    if term.is_some() {
        // Consume the closing quote (masked tag markup).
        *out.char_idx += 1;
        *out.current_utf16_col += ch.len_utf16() as u32;
        out.adjustments.push((*out.current_utf16_col, end_suffix));
    }
    true
}

/// Track HTML tag / attribute-value state so bound attributes are only
/// recognized at attribute position (inside a tag, not inside a quoted
/// value). Colons in attribute values (e.g. `href="mailto:x"`,
/// `style="color:red"`) or in text between tags (`10:30`) never satisfy
/// `html.in_tag && html.attr_string.is_none()`.
pub(super) fn track(ch: char, line_chars: &[char], char_idx: usize, html: &mut HtmlPos) {
    match html.attr_string {
        Some(q) if ch == q => html.attr_string = None,
        Some(_) => {}
        None => {
            if ch == '<' {
                // Enter a tag only when `<` begins an element
                // (next char names a tag or is `/`), not on a
                // stray `<` in text or a `< ` comparison.
                let next = line_chars.get(char_idx + 1);
                if next.is_none() || next.is_some_and(|c| c.is_ascii_alphabetic() || *c == '/') {
                    html.in_tag = true;
                }
            } else if ch == '>' {
                html.in_tag = false;
            } else if html.in_tag && (ch == '"' || ch == '\'') {
                html.attr_string = Some(ch);
            }
        }
    }
}
