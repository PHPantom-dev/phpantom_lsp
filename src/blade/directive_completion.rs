//! Detection for `@` directive-name completion.
//!
//! Answers one question: at a given byte offset in the *raw* Blade buffer,
//! is the cursor right after an `@` that starts a new directive name — i.e.
//! in plain template markup, not inside `{{ }}` / `{!! !!}`, a `{{-- --}}`
//! comment, a `@php` / `<?php` / `<?=` block, or `@verbatim`?
//!
//! This runs on the raw buffer rather than the preprocessed virtual PHP
//! (`src/blade/preprocessor.rs`) the rest of completion works from: an
//! incomplete directive name like `@i` doesn't match any known directive,
//! so the preprocessor masks it as inert HTML and it never survives into
//! the virtual PHP at all.

use super::directives::match_directive;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Html,
    /// Scanning toward a fixed closing token that is itself real PHP (an
    /// echo expression, a raw `<?php` tag, or a `@php` block body), so a
    /// string literal containing a lookalike token must not end the span
    /// early.
    UntilMarkerInCode(&'static str),
    /// Scanning toward a fixed closing token that is not PHP — a Blade
    /// comment, an `@`-escaped echo (`@{{ … }}` / `@{!! … !!}`), or a
    /// `@verbatim` block — so quotes inside it are just text.
    UntilMarkerRaw(&'static str),
}

/// Whether `content[..offset]` leaves the scanner in [`Mode::Html`], i.e.
/// `offset` itself sits in template markup rather than inside one of the
/// spans [`Mode`] tracks.
pub(crate) fn is_html_position(content: &str, offset: usize) -> bool {
    mode_at(content, offset) == Mode::Html
}

/// The [`Mode`] the scanner is in once it reaches `offset`, i.e. what the
/// compiler makes of everything up to (not including) `offset`.
///
/// This is the single scanner both the directive-completion and
/// echo-delimiter features read: it is what tells a `{{`/`}}`/`@{{` sighting
/// apart from a lookalike inside a `{{-- --}}` comment, a `@verbatim` block,
/// or an `@`-escaped echo, which none of those compile to an actual echo.
pub(crate) fn mode_at(content: &str, offset: usize) -> Mode {
    let mut mode = Mode::Html;
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    let mut i = 0usize;

    while i < offset {
        let Some(rest) = content.get(i..) else { break };
        let Some(ch) = rest.chars().next() else { break };
        let ch_len = ch.len_utf8();

        match mode {
            Mode::Html => {
                if rest.starts_with("{{--") {
                    mode = Mode::UntilMarkerRaw("--}}");
                    i += 4;
                } else if rest.starts_with("{!!") {
                    mode = Mode::UntilMarkerInCode("!!}");
                    i += 3;
                } else if rest.starts_with("{{") {
                    mode = Mode::UntilMarkerInCode("}}");
                    i += 2;
                } else if rest.starts_with("<?xml") {
                    // Never a PHP tag regardless of `short_open_tag`,
                    // mirroring the real preprocessor.
                    i += 5;
                } else if rest.starts_with("<?php") {
                    mode = Mode::UntilMarkerInCode("?>");
                    i += 5;
                } else if rest.starts_with("<?=") {
                    mode = Mode::UntilMarkerInCode("?>");
                    i += 3;
                } else if rest.starts_with("<?") {
                    mode = Mode::UntilMarkerInCode("?>");
                    i += 2;
                } else if let Some(after_at) = rest.strip_prefix('@') {
                    if after_at.starts_with("{!!") {
                        // `@{!! … !!}` escapes a raw Blade echo: the `@`
                        // strips the whole thing for a frontend template
                        // engine, so none of it is PHP.
                        mode = Mode::UntilMarkerRaw("!!}");
                        i += 1 + 3;
                    } else if after_at.starts_with("{{") {
                        mode = Mode::UntilMarkerRaw("}}");
                        i += 1 + 2;
                    } else if let Some(directive) = match_directive(after_at) {
                        i += 1 + directive.len();
                        if directive == "php" {
                            let after_directive = after_at[directive.len()..].trim_start();
                            if !after_directive.starts_with('(') {
                                mode = Mode::UntilMarkerInCode("@endphp");
                            }
                        } else if directive == "verbatim" {
                            mode = Mode::UntilMarkerRaw("@endverbatim");
                        }
                        // Every other directive's body (if it has one, e.g.
                        // `@if`/`@auth`/`@section`) is ordinary template
                        // markup up to its matching `@end...`, which is
                        // itself just another directive token scanned the
                        // same way — no mode change needed.
                    } else {
                        i += ch_len;
                    }
                } else {
                    i += ch_len;
                }
            }
            Mode::UntilMarkerInCode(marker) => {
                if let Some(quote) = in_string {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == quote {
                        in_string = None;
                    }
                    i += ch_len;
                } else if ch == '\'' || ch == '"' {
                    in_string = Some(ch);
                    i += ch_len;
                } else if rest.starts_with(marker) {
                    mode = Mode::Html;
                    i += marker.len();
                } else {
                    i += ch_len;
                }
            }
            Mode::UntilMarkerRaw(marker) => {
                if rest.starts_with(marker) {
                    mode = Mode::Html;
                    i += marker.len();
                } else {
                    i += ch_len;
                }
            }
        }
    }

    mode
}

/// The directive-name prefix already typed after an `@` the cursor sits
/// right after, when that `@` is in an HTML/directive position. Returns
/// `None` when the cursor isn't right after such an `@` — including a
/// literal `@@` escape, where the preceding character is itself `@`.
pub fn directive_prefix_at(content: &str, offset: usize) -> Option<&str> {
    let before = content.get(..offset)?;
    let name_start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric()))
        .map(|i| i + before[i..].chars().next().unwrap().len_utf8())
        .unwrap_or(0);

    let at_char_start = before[..name_start].rfind('@')?;
    if at_char_start + 1 != name_start {
        // Something other than an identifier character sits directly
        // between the `@` and the cursor — not a directive name in
        // progress.
        return None;
    }
    if before[..at_char_start].ends_with('@') {
        // `@@name` is Blade's escape for a literal `@name`, not a directive.
        return None;
    }

    if !is_html_position(content, at_char_start) {
        return None;
    }

    Some(&content[name_start..offset])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix_at<'a>(content: &'a str, marker: &str) -> Option<&'a str> {
        let offset = content.find(marker).expect("marker not found") + marker.len();
        directive_prefix_at(content, offset)
    }

    #[test]
    fn bare_at_in_html_offers_an_empty_prefix() {
        assert_eq!(prefix_at("<div>@|</div>", "@"), Some(""));
    }

    #[test]
    fn partial_directive_name_is_the_prefix() {
        assert_eq!(prefix_at("<div>@if|</div>", "@if"), Some("if"));
    }

    #[test]
    fn inside_echo_braces_does_not_trigger() {
        assert_eq!(prefix_at("{{ @|", "@"), None);
    }

    #[test]
    fn inside_raw_echo_braces_does_not_trigger() {
        assert_eq!(prefix_at("{!! @|", "@"), None);
    }

    #[test]
    fn inside_a_php_block_does_not_trigger() {
        assert_eq!(prefix_at("@php $x = 1; @|", "@php $x = 1; @"), None);
    }

    #[test]
    fn inside_a_raw_php_tag_does_not_trigger() {
        assert_eq!(prefix_at("<?php $x = 1; @| ?>", "$x = 1; @"), None);
    }

    #[test]
    fn inside_verbatim_does_not_trigger() {
        assert_eq!(prefix_at("@verbatim @| @endverbatim", "@verbatim @"), None);
    }

    #[test]
    fn after_verbatim_ends_it_triggers_again() {
        assert_eq!(
            prefix_at("@verbatim x @endverbatim @|", "@endverbatim @"),
            Some("")
        );
    }

    #[test]
    fn a_string_containing_a_lookalike_closer_does_not_end_the_echo_early() {
        // The `}}` inside the string must not be treated as the closing
        // `}}` of the echo — the real one is further along.
        assert_eq!(prefix_at(r#"{{ "a}}b" }} @|"#, "}} @"), Some(""));
    }

    #[test]
    fn a_string_containing_endphp_does_not_end_the_php_block_early() {
        assert_eq!(
            prefix_at(r#"@php $x = "@endphp"; @|"#, "@endphp\"; @"),
            None
        );
    }

    #[test]
    fn escaped_at_sign_does_not_trigger() {
        assert_eq!(prefix_at("<div>@@i|</div>", "@@i"), None);
    }

    #[test]
    fn a_directive_body_is_still_html_position() {
        // `@if (...)` opens a block, but the markup between it and its
        // `@end...` is ordinary template content — directive completion
        // must still fire inside it.
        assert_eq!(
            prefix_at("@if ($x)\n    @|\n@endif", "@if ($x)\n    @"),
            Some("")
        );
    }

    #[test]
    fn xml_declaration_is_not_mistaken_for_a_php_tag() {
        assert_eq!(prefix_at("<?xml version=\"1.0\" ?>@|", "?>@"), Some(""));
    }

    #[test]
    fn not_right_after_an_at_sign_does_not_trigger() {
        assert_eq!(directive_prefix_at("<div>text|</div>", 9), None);
    }
}
