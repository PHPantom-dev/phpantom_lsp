//! Where an offset sits in PHP source, resolved by a single forward scan.
//!
//! Completion needs three things about the text leading up to a cursor: the
//! string literal the cursor is typing in, if any (so the key in
//! `route('name', ['us|` is known to start after that quote), the last byte of
//! real code before it (so that key is still seen to follow a `[` across
//! `route('name', [ // note (here)\n '|'`), and the chain of brackets left
//! open at it (so the `[` of an argument array and the `(` of the call it
//! belongs to can be told apart from any other nesting).  Each open bracket
//! carries the commas seen inside it and where the code before it ends, which
//! is the argument index and the callee of the call it opens, plus the
//! `->` / `?->` / `::` immediately before that callee, if any, so the
//! receiver in front of it can be read from a comment-free boundary too.
//!
//! All of it comes out of one forward pass.  A backwards walk cannot answer
//! any of them: read from the right, a `//`, a `#`, or a quote is ambiguous
//! (it may open a comment, sit inside a string, or close one), so a stray
//! bracket or apostrophe inside a comment unbalances the walk.  Scanning
//! forward the lexical state is unambiguous, and tracking the open brackets
//! on the way gives the enclosing constructs directly.
//!
//! Starting from the top of the file is what makes that unambiguous, so the
//! pass costs the file: a caller resolves it **once** per request and shares
//! the result with every strategy that needs it (see
//! `Backend::handle_completion`), rather than each one scanning for itself.

use memchr::{memchr, memchr2, memmem};

/// A bracket left open at the offset, with what the scan saw around it.
pub(crate) struct OpenBracket {
    /// Byte offset of the opening bracket.
    pub(crate) offset: usize,
    /// Which bracket it is: `(`, `[`, or `{`.
    pub(crate) byte: u8,
    /// One past the last byte of code before the bracket, so
    /// `&content[..code_before]` is the text leading up to it with trailing
    /// whitespace and comments cut off — the callee of a call, and the
    /// receiver before that, are read from its end.
    pub(crate) code_before: usize,
    /// Commas seen directly inside this bracket, which is the index of the
    /// argument or element the offset sits in.  A comma of a nested bracket
    /// belongs to that bracket, and one in a comment or a literal is not
    /// code, so neither is counted here.
    pub(crate) commas: usize,
    /// Start of the current argument or array element, after the opener or
    /// most recent comma. Whitespace and comments may follow.
    pub(crate) item_start: usize,
    /// Whether this item contains an arrow function at this bracket depth,
    /// whose `=>` must not be mistaken for an associative-array arrow.
    pub(crate) item_has_arrow_function: bool,
    /// The `->`, `?->`, or `::` immediately before this bracket's callee,
    /// when it is a `(` that opens a method or static call.  `None` for a
    /// plain function call, or for a `[`/`{`.
    pub(crate) callee_operator: Option<Operator>,
    /// Byte offset where this bracket's callee identifier starts, alongside
    /// `callee_operator`.  Kept separately from `callee_operator.code_before`
    /// (which ends *before* the identifier) so a later hop can read the
    /// identifier's own text.
    pub(crate) callee_name_start: Option<usize>,
}

/// A `->`, `?->`, or `::` seen while scanning code, naming where the
/// receiver in front of it ends.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Operator {
    /// One past the last byte of code before the operator, so
    /// `&content[..code_before]` is the receiver with trailing whitespace
    /// and comments cut off.
    pub(crate) code_before: usize,
    /// `true` for `::`, `false` for `->` and `?->`.
    pub(crate) is_static: bool,
    /// Set when the text immediately before this operator is the closing
    /// `)` of a method/static call, e.g. the `->` before `only` in
    /// `$request->safe()->only(…)`.  Lets a consumer look through that one
    /// call to the receiver beneath it, and identify the call by name.
    pub(crate) hop: Option<Hop>,
}

/// A call whose closing `)` sits immediately (modulo whitespace/comments)
/// before an operator, letting that operator's receiver be traced through it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hop {
    /// One past the last byte of code before the hopped-through call's own
    /// receiver, e.g. where `$request` ends in `$request->safe()->only(…)`.
    pub(crate) code_before: usize,
    /// Byte range of the hopped-through call's method name, e.g. `safe`.
    pub(crate) name_start: usize,
    pub(crate) name_end: usize,
}

/// The lexical position of an offset in PHP source.
pub(crate) struct CodeContext<'a> {
    /// The source text up to and including the last byte of code before the
    /// offset, with trailing whitespace and comments cut off.  A
    /// `ends_with` / `strip_suffix` check on it therefore sees code.
    pub(crate) code_before: &'a str,
    /// The brackets still open at the offset, outermost first.
    pub(crate) open_brackets: Vec<OpenBracket>,
    /// Set when the offset sits inside a `'…'` or `"…"` literal, holding the
    /// offset and character of its opening quote.  The two fields above then
    /// describe where that quote sits, since nothing inside a literal opens a
    /// bracket or counts as code.
    pub(crate) open_string: Option<(usize, char)>,
}

impl CodeContext<'_> {
    /// The last byte of code before the offset, or `None` when nothing but
    /// whitespace and comments precedes it.
    pub(crate) fn last_code_byte(&self) -> Option<u8> {
        self.code_before.as_bytes().last().copied()
    }

    /// The offset of the innermost open bracket and of the one enclosing it,
    /// when they are `expected_inner` nested directly in `expected_outer`.
    ///
    /// `route('users.show', ['|'` asked for `[` in `(` yields the offsets of
    /// that `[` and that `(`; a key of a nested array (`['a' => ['|'`) has an
    /// `[` around it instead and yields `None`.
    pub(crate) fn nested_pair(
        &self,
        expected_inner: u8,
        expected_outer: u8,
    ) -> Option<(usize, usize)> {
        match &self.open_brackets[..] {
            [.., outer, inner] if inner.byte == expected_inner && outer.byte == expected_outer => {
                Some((inner.offset, outer.offset))
            }
            _ => None,
        }
    }

    /// The innermost `(` left open at the offset.
    ///
    /// For an offset in a call's argument list this is the `(` that opens it,
    /// whether the offset sits in the list directly (`foo('|')`) or inside an
    /// argument's array (`foo(['|'])`), and its comma count is the index of
    /// the argument the offset belongs to.  A `(` opens other constructs too
    /// (a condition, a grouped expression), so callers that need a call check
    /// the callee before the paren.
    pub(crate) fn enclosing_paren(&self) -> Option<&OpenBracket> {
        self.open_brackets.iter().rev().find(|b| b.byte == b'(')
    }
}

/// What the scan is in the middle of.  Nowdocs behave like heredocs here:
/// both are opaque bodies closed by their label.
enum State {
    /// Inline HTML, outside any `<?php` block.
    Html,
    Code,
    /// Inside a `'…'` literal opened at this offset.
    SingleString(usize),
    /// Inside a `"…"` literal opened at this offset.
    DoubleString(usize),
    /// `// …` or `# …`.
    LineComment,
    /// `/* … */`, docblocks included.
    BlockComment,
    Heredoc,
}

/// Resolve the lexical position of `offset` in `content`.
///
/// Returns `None` when `offset` is in a comment, a heredoc body, or inline
/// HTML, because the bracket nesting recorded for such a position says nothing
/// about the expression the cursor is in.  An offset inside a string literal
/// *is* reported, with [`CodeContext::open_string`] naming the literal it sits
/// in: that is where the cursor sits while a string argument or array key is
/// being typed, and the brackets around the literal are the ones its call and
/// argument array need.
///
/// The scan starts in inline HTML when an opening tag appears before
/// `offset`, and in code otherwise, so a bare fragment with no `<?php` (as
/// tests and synthesised snippets use) is read as code throughout.
pub(crate) fn code_context_at(content: &str, offset: usize) -> Option<CodeContext<'_>> {
    let bytes = content.as_bytes();
    let end = offset;
    if end > bytes.len() {
        return None;
    }

    let mut state = if memmem::find(&bytes[..end], b"<?").is_some() {
        State::Html
    } else {
        State::Code
    };
    let mut open_brackets: Vec<OpenBracket> = Vec::new();
    // One past the last byte of code seen so far.
    let mut code_end = 0usize;
    let mut heredoc_label: &[u8] = &[];
    let mut i = 0usize;
    // The most recent `->` / `?->` / `::` since the last time it was
    // invalidated, and whether the identifier run that is its callee has
    // been seen yet.  A second identifier run (e.g. the `bar` of `$a->foo
    // and bar(`), a bracket, or a comma all invalidate it, since none of
    // those can follow a call's callee.
    let mut current_op: Option<Operator> = None;
    let mut op_callee_seen = false;
    // Byte offset where `current_op`'s callee identifier started, once seen.
    let mut current_op_name_start: Option<usize> = None;
    // The hop through the call that a `)` just closed, valid only while
    // nothing but whitespace/comments follows it — the same invalidation
    // rules as `current_op`, since a new operator right after is what
    // consumes it.
    let mut last_closed_call: Option<Hop> = None;

    while i < end {
        match state {
            State::Html => match memmem::find(&bytes[i..end], b"<?") {
                Some(pos) => {
                    i += pos + 2;
                    if bytes[i..end].starts_with(b"php") {
                        i += 3;
                    } else if bytes[i..end].starts_with(b"=") {
                        i += 1;
                    }
                    state = State::Code;
                }
                None => i = end,
            },
            State::LineComment => {
                // The comment ends at the newline, or earlier at a `?>`,
                // which closes the PHP block even inside a comment.
                let newline = memchr(b'\n', &bytes[i..end]);
                let line_end = newline.map_or(end, |pos| i + pos + 1);
                match memmem::find(&bytes[i..line_end], b"?>") {
                    Some(pos) => {
                        i += pos + 2;
                        state = State::Html;
                    }
                    None => {
                        i = line_end;
                        if newline.is_some() {
                            state = State::Code;
                        }
                    }
                }
            }
            State::BlockComment => match memmem::find(&bytes[i..end], b"*/") {
                Some(pos) => {
                    i += pos + 2;
                    state = State::Code;
                }
                None => i = end,
            },
            State::SingleString(_) | State::DoubleString(_) => {
                let quote = if matches!(state, State::SingleString(_)) {
                    b'\''
                } else {
                    b'"'
                };
                match memchr2(quote, b'\\', &bytes[i..end]) {
                    Some(pos) if bytes[i + pos] == b'\\' => {
                        // The byte after the backslash is escaped and cannot
                        // close the literal.  Only ASCII escapes exist, so
                        // stepping over a multi-byte character is never
                        // needed (and would land mid-character).
                        i += pos + 1;
                        if i < end && bytes[i].is_ascii() {
                            i += 1;
                        }
                    }
                    Some(pos) => {
                        i += pos + 1;
                        code_end = i;
                        state = State::Code;
                    }
                    None => i = end,
                }
            }
            State::Heredoc => {
                // The body runs to a line whose first non-blank text is the
                // label, followed by anything that cannot continue an
                // identifier.
                let Some(newline) = memchr(b'\n', &bytes[i..end]) else {
                    i = end;
                    continue;
                };
                let mut j = i + newline + 1;
                while j < end && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                if bytes[j..end].starts_with(heredoc_label) {
                    let after = j + heredoc_label.len();
                    if after >= end
                        || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'_')
                    {
                        code_end = after;
                        i = after;
                        state = State::Code;
                        continue;
                    }
                }
                i = j;
            }
            State::Code => {
                let byte = bytes[i];
                match byte {
                    b'/' if bytes[i + 1..end].starts_with(b"/") => {
                        state = State::LineComment;
                        i += 2;
                        continue;
                    }
                    b'/' if bytes[i + 1..end].starts_with(b"*") => {
                        state = State::BlockComment;
                        i += 2;
                        continue;
                    }
                    // `#[` opens an attribute; a bare `#` opens a comment.
                    b'#' if !bytes[i + 1..end].starts_with(b"[") => {
                        state = State::LineComment;
                        i += 1;
                        continue;
                    }
                    b'?' if bytes[i + 1..end].starts_with(b">") => {
                        state = State::Html;
                        i += 2;
                        continue;
                    }
                    b'?' if bytes[i + 1..end].starts_with(b"->") => {
                        current_op = Some(Operator {
                            code_before: code_end,
                            is_static: false,
                            hop: last_closed_call.take(),
                        });
                        op_callee_seen = false;
                        current_op_name_start = None;
                        code_end = i + 3;
                        i += 3;
                        continue;
                    }
                    b'-' if bytes[i + 1..end].starts_with(b">") => {
                        current_op = Some(Operator {
                            code_before: code_end,
                            is_static: false,
                            hop: last_closed_call.take(),
                        });
                        op_callee_seen = false;
                        current_op_name_start = None;
                        code_end = i + 2;
                        i += 2;
                        continue;
                    }
                    b':' if bytes[i + 1..end].starts_with(b":") => {
                        current_op = Some(Operator {
                            code_before: code_end,
                            is_static: true,
                            hop: last_closed_call.take(),
                        });
                        op_callee_seen = false;
                        current_op_name_start = None;
                        code_end = i + 2;
                        i += 2;
                        continue;
                    }
                    b'\'' => {
                        state = State::SingleString(i);
                        i += 1;
                        continue;
                    }
                    b'"' => {
                        state = State::DoubleString(i);
                        i += 1;
                        continue;
                    }
                    b'<' if bytes[i..end].starts_with(b"<<<") => {
                        if let Some((label, body)) = heredoc_opener(bytes, i, end) {
                            heredoc_label = label;
                            state = State::Heredoc;
                            i = body;
                            continue;
                        }
                    }
                    b'(' | b'[' | b'{' => {
                        // Only a `(` can open a call, so only it takes the
                        // pending operator (and its callee's name start) as
                        // its callee's receiver; either way a fresh bracket
                        // starts a new scope for it.
                        let (callee_operator, callee_name_start) = if byte == b'(' {
                            (current_op.take(), current_op_name_start.take())
                        } else {
                            (None, None)
                        };
                        current_op = None;
                        current_op_name_start = None;
                        last_closed_call = None;
                        open_brackets.push(OpenBracket {
                            offset: i,
                            byte,
                            code_before: code_end,
                            commas: 0,
                            item_start: i + 1,
                            item_has_arrow_function: false,
                            callee_operator,
                            callee_name_start,
                        });
                    }
                    b')' | b']' | b'}' => {
                        let opener = match byte {
                            b')' => b'(',
                            b']' => b'[',
                            _ => b'{',
                        };
                        last_closed_call = None;
                        // A closer that does not match the innermost opener
                        // belongs to an unbalanced edit; dropping it keeps
                        // the brackets that do pair up intact.
                        if let Some(popped) = open_brackets.pop_if(|b| b.byte == opener) {
                            // A call that just closed right before an
                            // operator lets that operator's receiver be
                            // traced through it (the `safe()` hop).
                            if byte == b')'
                                && let Some(op) = popped.callee_operator
                                && let Some(name_start) = popped.callee_name_start
                            {
                                last_closed_call = Some(Hop {
                                    code_before: op.code_before,
                                    name_start,
                                    name_end: popped.code_before,
                                });
                            }
                        }
                        current_op = None;
                    }
                    b',' => {
                        if let Some(bracket) = open_brackets.last_mut() {
                            bracket.commas += 1;
                            bracket.item_start = i + 1;
                            bracket.item_has_arrow_function = false;
                        }
                        current_op = None;
                        last_closed_call = None;
                    }
                    _ if byte.is_ascii_alphanumeric() || byte == b'_' => {
                        // The start of an identifier run: the first one after
                        // an operator is its callee, so it keeps the operator
                        // alive (recording where it starts); a second (e.g.
                        // `and`/`or`, or an unrelated name) means the callee
                        // was never called and the operator does not belong
                        // to whatever comes next.
                        let word_start = i == 0
                            || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
                        if word_start {
                            if current_op.is_some() {
                                if op_callee_seen {
                                    current_op = None;
                                } else {
                                    op_callee_seen = true;
                                    current_op_name_start = Some(i);
                                }
                            }
                            last_closed_call = None;
                        }
                        // Only the first byte of the run carries meaning, so
                        // step over the rest in one go rather than re-entering
                        // the dispatch for every character of every name.
                        let start = i;
                        while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                            i += 1;
                        }
                        if bytes[start..i].eq_ignore_ascii_case(b"fn")
                            && current_op.is_none()
                            && (start == 0
                                || (bytes[start - 1] != b'$' && bytes[start - 1].is_ascii()))
                            && bytes.get(i).is_none_or(u8::is_ascii)
                            && let Some(bracket) = open_brackets.last_mut()
                        {
                            bracket.item_has_arrow_function = true;
                        }
                        code_end = i;
                        continue;
                    }
                    _ if !byte.is_ascii_whitespace() => {
                        current_op = None;
                        last_closed_call = None;
                    }
                    _ => {
                        // Whitespace neither ends the code seen so far nor
                        // invalidates a pending operator, so a run of it can be
                        // skipped whole.
                        while i < end && bytes[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        continue;
                    }
                }
                code_end = i + 1;
                i += 1;
            }
        }
    }

    let open_string = match state {
        State::Code => None,
        State::SingleString(quote) => Some((quote, '\'')),
        State::DoubleString(quote) => Some((quote, '"')),
        State::Html | State::LineComment | State::BlockComment | State::Heredoc => return None,
    };
    Some(CodeContext {
        code_before: content.get(..code_end)?,
        open_brackets,
        open_string,
    })
}

/// Read a `<<<LABEL` heredoc/nowdoc opener starting at `start`.
///
/// Returns the label and the offset the body scan should resume from (the
/// rest of the opener's line, which the [`State::Heredoc`] arm skips to find
/// its newline).  Returns `None` when no label follows, which means the
/// `<<<` is not an opener.
fn heredoc_opener(bytes: &[u8], start: usize, end: usize) -> Option<(&[u8], usize)> {
    let mut i = start + 3;
    while i < end && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    let quoted = i < end && (bytes[i] == b'\'' || bytes[i] == b'"');
    if quoted {
        i += 1;
    }
    let label_start = i;
    while i < end && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if i == label_start {
        return None;
    }
    let label = &bytes[label_start..i];
    if quoted && i < end && (bytes[i] == b'\'' || bytes[i] == b'"') {
        i += 1;
    }
    Some((label, i))
}

#[cfg(test)]
#[path = "code_context_tests.rs"]
mod tests;
