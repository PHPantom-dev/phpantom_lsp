use super::Mode;
use super::component_call::contains_seq;
use super::shared::Lowering;
use crate::blade::directives::{CustomDirectives, match_directive};

/// The last line holding each echo terminator, computed once so an echo
/// opener can ask "is there a terminator anywhere after me?" without
/// rescanning the rest of the file per opener (an opener with none would
/// otherwise cost O(file) each, O(file²) across a file of them).
pub(super) struct EchoCloses {
    pub(super) escaped: Option<usize>,
    pub(super) raw: Option<usize>,
}

/// An echo, a raw echo, or a Blade comment opening at the cursor.
pub(super) fn open(
    remaining: &[char],
    line_idx: usize,
    closes: &EchoCloses,
    echo_closes_at_eol: &mut bool,
) -> Option<Lowering> {
    let match_len;
    let replacement;
    let next_mode;

    if remaining.starts_with(&['{', '{']) && !remaining[1..].starts_with(&['{', '!', '!']) {
        let is_comment = remaining.starts_with(&['{', '{', '-', '-']);
        replacement = if is_comment {
            " /* ".to_string()
        } else {
            " echo e(".to_string()
        };
        match_len = if is_comment { 4 } else { 2 };
        next_mode = if is_comment {
            Mode::Comment
        } else {
            *echo_closes_at_eol = !contains_seq(&remaining[2..], &['}', '}'])
                && closes.escaped.is_none_or(|last| last <= line_idx);
            Mode::Php(false)
        };
    } else if remaining.starts_with(&['{', '!', '!']) {
        // `{!! … !!}` outputs unescaped, so it compiles to a
        // naked `echo` with no `e()` wrapper. Blade matches its
        // echo tags longest-opening-first, so in `{{!! … !!}}`
        // the raw echo starts at the second `{` and the outer
        // braces are literal text — the guard above keeps the
        // first `{` from being read as an escaped echo instead.
        replacement = " echo ".to_string();
        match_len = 3;
        next_mode = Mode::Php(true);
        *echo_closes_at_eol = !contains_seq(&remaining[3..], &['!', '!', '}'])
            && closes.raw.is_none_or(|last| last <= line_idx);
    } else {
        return None;
    }

    Some(Lowering {
        match_len,
        replacement,
        next_mode,
    })
}

/// An `@`-escaped echo opening at the cursor.
pub(super) fn open_escaped(
    remaining: &[char],
    line_idx: usize,
    closes: &EchoCloses,
    echo_closes_at_eol: &mut bool,
) -> Option<Lowering> {
    if !remaining.starts_with(&['@', '{', '{']) && !remaining.starts_with(&['@', '{', '!', '!']) {
        return None;
    }

    // The `@` escapes the complete Blade echo for a frontend template
    // engine. Mask everything through its closing delimiter rather than
    // exposing the expression as PHP.
    let raw = remaining[2] == '!';
    *echo_closes_at_eol = if raw {
        !contains_seq(&remaining[4..], &['!', '!', '}'])
            && closes.raw.is_none_or(|last| last <= line_idx)
    } else {
        !contains_seq(&remaining[3..], &['}', '}'])
            && closes.escaped.is_none_or(|last| last <= line_idx)
    };

    Some(Lowering {
        match_len: if raw { 4 } else { 3 },
        replacement: String::new(),
        next_mode: Mode::EscapedEcho(raw),
    })
}

/// What ends an `@`-escaped echo.
pub(super) fn close_escaped(
    raw: bool,
    remaining: &[char],
    custom_directives: &CustomDirectives,
) -> Lowering {
    let mut match_len = 0;
    let replacement = String::new();
    let mut next_mode = Mode::EscapedEcho(raw);

    let closes_echo = if raw {
        remaining.starts_with(&['!', '!', '}'])
    } else {
        remaining.starts_with(&['}', '}'])
    };
    if closes_echo {
        match_len = if raw { 3 } else { 2 };
        next_mode = Mode::Html;
    } else if directive_boundary(remaining, custom_directives) {
        next_mode = Mode::Html;
    }

    Lowering {
        match_len,
        replacement,
        next_mode,
    }
}

/// What ends a Blade comment.
pub(super) fn close_comment(remaining: &[char], line_chars: &[char], char_idx: usize) -> Lowering {
    let mut match_len = 0;
    let mut replacement = String::new();
    let mut next_mode = Mode::Comment;

    // Inside a comment the only meaningful token is the `--}}`
    // terminator, which Blade requires to be contiguous. Comment
    // text is neither PHP nor Blade, so a commented-out echo's
    // `}}`/`!!}` and an `@endphp` written in prose must not end
    // it — treating either as the terminator would leave the
    // emitted `/*` open and desync the rest of the file.
    if remaining.starts_with(&['}', '}'])
        && char_idx >= 2
        && line_chars[char_idx - 2..].starts_with(&['-', '-'])
    {
        replacement = " */ ".to_string();
        match_len = 2;
        next_mode = Mode::Html;
    }

    Lowering {
        match_len,
        replacement,
        next_mode,
    }
}

/// What ends the PHP an echo or an `@php` block opened.
pub(super) fn close_php(
    raw_echo: bool,
    remaining: &[char],
    in_php_directive_block: &mut bool,
    custom_directives: &CustomDirectives,
) -> Lowering {
    let mut match_len = 0;
    let mut replacement = String::new();
    let mut next_mode = Mode::Php(raw_echo);

    // Each echo form only closes at its own terminator: `!!}`
    // ends a raw echo and `}}` an escaped one, exactly as
    // Blade's compiler matches them. A raw echo opened a bare
    // `echo ` with no `e(`, so there is no call to close, only
    // the statement.
    if raw_echo && remaining.starts_with(&['!', '!', '}']) {
        replacement = "; ".to_string();
        match_len = 3;
        next_mode = Mode::Html;
    } else if !raw_echo && remaining.starts_with(&['}', '}']) {
        replacement = "); ".to_string();
        match_len = 2;
        next_mode = Mode::Html;
    } else if remaining.starts_with(&['@', 'e', 'n', 'd', 'p', 'h', 'p']) {
        *in_php_directive_block = false;
        next_mode = Mode::Html;
        match_len = 7;
        replacement = "".to_string();
    } else if !*in_php_directive_block && directive_boundary(remaining, custom_directives) {
        replacement = if raw_echo {
            "; ".to_string()
        } else {
            "); ".to_string()
        };
        next_mode = Mode::Html;
    }

    Lowering {
        match_len,
        replacement,
        next_mode,
    }
}

/// A directive appearing while an echo or escaped echo is still open
/// ends it. Blade's statement compiler runs before the echo compiler, so
/// by the time echo compilation would see a directive, Blade has already
/// turned it into real PHP (or, for an escaped echo, the directive was
/// never part of the frontend-only text to begin with). Absorbing the
/// directive as part of the echo instead leaves whatever block it closes
/// (`@endif`, `@endforeach`, ...) unclosed in the emitted PHP.
fn directive_boundary(remaining: &[char], custom_directives: &CustomDirectives) -> bool {
    remaining.first() == Some(&'@') && {
        let rest: String = remaining[1..].iter().collect();
        match_directive(&rest).is_some() || custom_directives.match_directive(&rest).is_some()
    }
}
