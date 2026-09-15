use super::Mode;

/// The lowering an arm produces for the construct it matched: how much
/// of the Blade source it consumes, the PHP emitted in its place, and
/// the mode the scan continues in.
pub(super) struct Lowering {
    pub(super) match_len: usize,
    pub(super) replacement: String,
    pub(super) next_mode: Mode,
}

impl Lowering {
    /// Nothing matched at the cursor, so the character is ordinary
    /// content.
    pub(super) fn keep(mode: Mode) -> Self {
        Self {
            match_len: 0,
            replacement: String::new(),
            next_mode: mode,
        }
    }
}

/// The line being lowered: the PHP emitted for it so far, the source
/// text not yet flushed into that, the Blade-to-PHP column map, and the
/// cursor into the line.
pub(super) struct LineOut<'a> {
    pub(super) processed: &'a mut String,
    pub(super) buffer: &'a mut String,
    pub(super) adjustments: &'a mut Vec<(u32, u32)>,
    pub(super) char_idx: &'a mut usize,
    pub(super) current_utf16_col: &'a mut u32,
}

impl LineOut<'_> {
    /// Append PHP that has no Blade source of its own (a statement
    /// terminator, a synthesized call) and anchor both its ends to the
    /// current Blade column, so the map keeps the text around it aligned.
    /// Returns the emitted end offset, for callers that need to anchor
    /// further masked source to it.
    pub(super) fn emit_suffix(&mut self, text: &str) -> u32 {
        let start_suffix = utf16_count(self.processed) as u32;
        self.processed.push_str(text);
        let end_suffix = utf16_count(self.processed) as u32;

        self.adjustments
            .push((*self.current_utf16_col, start_suffix));
        self.adjustments.push((*self.current_utf16_col, end_suffix));

        end_suffix
    }
}

/// Track a directive argument list's paren nesting, reporting whether `ch`
/// is the `)` that balances it.
pub(super) fn closes_args(ch: char, paren_depth: &mut i32) -> bool {
    if ch == '(' {
        *paren_depth += 1;
    } else if ch == ')' {
        *paren_depth -= 1;
        return *paren_depth <= 0;
    }
    false
}

pub(super) fn flush_buffer(
    processed: &mut String,
    buffer: &mut String,
    mode: Mode,
    current_utf16_col: u32,
    adjustments: &mut Vec<(u32, u32)>,
) {
    if buffer.is_empty() {
        return;
    }
    let blade_start = current_utf16_col.saturating_sub(utf16_count(buffer) as u32);

    if matches!(mode, Mode::Html | Mode::EscapedEcho(_)) {
        // HTML and frontend-template expressions are not PHP. Mask them with
        // spaces to maintain 1:1 utf-16 mapping.
        adjustments.push((blade_start, utf16_count(processed) as u32));

        for c in buffer.chars() {
            let len = c.len_utf16();
            for _ in 0..len {
                processed.push(' ');
            }
        }

        adjustments.push((current_utf16_col, utf16_count(processed) as u32));
    } else {
        // PHP content — 1:1 mapping
        adjustments.push((blade_start, utf16_count(processed) as u32));
        if mode == Mode::Comment {
            push_comment_text(processed, buffer);
        } else {
            processed.push_str(buffer);
        }
        adjustments.push((current_utf16_col, utf16_count(processed) as u32));
    }

    buffer.clear();
}

/// Copy Blade comment text into the emitted `/* ... */` block, blanking the
/// `/` of any `*/` in it. A literal `*/` in the text (common, since
/// commenting out a block of PHP is the usual reason to write a Blade
/// comment) would close the block early and turn the remainder of the
/// comment into live PHP. Replacing one character with a space rather than
/// escaping the sequence keeps the utf-16 columns aligned with the Blade
/// source.
fn push_comment_text(processed: &mut String, buffer: &str) {
    let mut after_star = false;
    for c in buffer.chars() {
        if after_star && c == '/' {
            processed.push(' ');
            after_star = false;
            continue;
        }
        after_star = c == '*';
        processed.push(c);
    }
}

pub(super) fn utf16_count(s: &str) -> usize {
    s.encode_utf16().count()
}
