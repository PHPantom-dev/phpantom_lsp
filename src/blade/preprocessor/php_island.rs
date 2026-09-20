use super::Mode;
use super::shared::Lowering;

/// A raw `<?php` / `<?=` / `<?` tag opening at the cursor.
pub(super) fn open(remaining: &[char]) -> Option<Lowering> {
    let mut match_len = 0;
    let mut replacement = String::new();
    let mut next_mode = Mode::Html;

    if remaining.starts_with(&['<', '?', 'p', 'h', 'p']) {
        // Raw <?php tag embedded directly in the template (not via @php).
        match_len = 5;
        next_mode = Mode::RawPhp(false);
    } else if remaining.starts_with(&['<', '?', '=']) {
        match_len = 3;
        replacement = " echo ".to_string();
        next_mode = Mode::RawPhp(true);
    } else if remaining.starts_with(&['<', '?', 'x', 'm', 'l']) {
        // `<?xml ... ?>` is never a PHP open tag, regardless of
        // `short_open_tag` — PHP special-cases it so XML
        // declarations in templates aren't misparsed. Leave it
        // as plain HTML.
    } else if remaining.starts_with(&['<', '?']) {
        match_len = 2;
        next_mode = Mode::RawPhp(false);
    } else {
        return None;
    }

    Some(Lowering {
        match_len,
        replacement,
        next_mode,
    })
}

/// The `?>` that ends a raw PHP tag.
pub(super) fn close(needs_semicolon: bool, remaining: &[char]) -> Lowering {
    let mut match_len = 0;
    let mut replacement = String::new();
    let mut next_mode = Mode::RawPhp(needs_semicolon);

    if remaining.starts_with(&['?', '>']) {
        replacement = if needs_semicolon {
            "; ".to_string()
        } else {
            "".to_string()
        };
        match_len = 2;
        next_mode = Mode::Html;
    }

    Lowering {
        match_len,
        replacement,
        next_mode,
    }
}
