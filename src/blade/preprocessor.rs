use super::directives::{match_directive, translate_directive};
use super::source_map::BladeSourceMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Html,
    Php,
    /// A raw `<?php` / `<?=` / `<?` tag embedded directly in the template
    /// (i.e. not via `@php`/`@endphp`). Content is passed through verbatim
    /// with no directive/echo scanning, and the mode ends at `?>`. The
    /// `bool` tracks whether the opening tag was a short-echo tag (`<?=`),
    /// which needs a trailing `;` injected before the closing `?>`.
    RawPhp(bool),
    DirectiveArgs(&'static str),
    SkipArgs(&'static str),
    Verbatim,
    /// The expression of a Blade component bound attribute
    /// (`:name="$expr"` or the `:$var` shorthand). The expression is
    /// emitted verbatim as a real PHP argument to `blade_directive(...)`
    /// so the forward walker sees the variables it uses; the surrounding
    /// tag markup stays masked. `Some(quote)` is the delimiting quote of
    /// a `:name="..."` value; `None` is the shorthand `:$var`, which ends
    /// at the first character that cannot be part of the variable name.
    BoundAttr(Option<char>),
    /// The parenthesised argument list of an `@use(...)` or `@inject(...)`
    /// directive. Unlike `DirectiveArgs`, the argument text is captured and
    /// transformed (rather than emitted verbatim) so the correct real PHP
    /// construct can be produced when the list closes.
    CaptureArgs(CapturedDirective),
}

/// Which directive is having its argument list captured by
/// [`Mode::CaptureArgs`]. The two have different real-PHP translations:
/// `@use` becomes a top-level `use` import (hoisted out of the wrapper
/// function), `@inject` becomes an inline `$var = app(service);` assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapturedDirective {
    Use,
    Inject,
}

pub fn preprocess(content: &str) -> (String, BladeSourceMap) {
    let mut virtual_php = String::with_capacity(content.len() + 512);
    let mut source_map = BladeSourceMap::default();

    // ── Prologue (5 lines) ──
    virtual_php.push_str("<?php if (!function_exists('blade_directive')) { function blade_directive(...$args) {} function blade_view_directive(...$args) {} }\n");
    virtual_php.push_str("/** @var \\Illuminate\\Support\\ViewErrorBag $errors */\n");
    virtual_php.push_str("$errors = new \\Illuminate\\Support\\ViewErrorBag();\n");
    virtual_php.push_str("/** @var \\Illuminate\\View\\Factory $__env */\n");
    virtual_php.push_str("$__env = new \\Illuminate\\View\\Factory();\n");
    // Wrap the template body in a function so that diagnostic
    // collectors (which only analyse function/method bodies) treat
    // the Blade content as analysable code.  The closing brace is
    // appended after the main loop.  `$errors`/`$__env` are assigned
    // in the outer scope above, so pull them in with `global` —
    // otherwise every use of them inside the wrapped function is a
    // false-positive "undefined variable".
    virtual_php.push_str("function __blade_template() { global $errors, $__env;\n");

    // `@use` imports cannot be emitted inline: the template body is wrapped
    // in `function __blade_template()`, and PHP `use` imports are only valid
    // at the top level. They are collected here and appended as real
    // top-level `use` statements after the wrapper function closes, so the
    // imported names populate the file's use-map and resolve.
    let mut hoisted_uses: Vec<String> = Vec::new();

    let mut in_php_directive_block = false;
    let mut mode = Mode::Html;
    let mut paren_depth = 0;
    let mut in_string: Option<char> = None;
    let mut is_escaped = false;
    // Whether the HTML scanner is currently between the `<` and `>` of a
    // tag, and (when inside a tag) whether it is inside a quoted attribute
    // value. Both persist across lines so multi-line tags are tracked
    // correctly. They gate recognition of `:name="$expr"` bound
    // attributes, which are only valid at attribute position inside a tag.
    let mut in_html_tag = false;
    let mut html_attr_string: Option<char> = None;

    for line in content.lines() {
        let mut processed = String::new();
        let mut adjustments = vec![(0, 0)]; // (blade_utf16_col, php_utf16_col)

        let mut current_utf16_col = 0;
        let line_chars: Vec<char> = line.chars().collect();
        let mut buffer = String::new();

        if mode == Mode::Html && in_php_directive_block {
            mode = Mode::Php;
        }

        let mut char_idx = 0;
        while char_idx < line_chars.len() {
            let ch = line_chars[char_idx];

            // Close a bound-attribute expression when its terminator is
            // reached. This must run before the generic string tracking
            // below, otherwise the closing `"` of a `:name="..."` value
            // would be mistaken for the start of a PHP string literal.
            if let Mode::BoundAttr(term) = mode {
                let at_end = match term {
                    Some(delim) => in_string.is_none() && ch == delim,
                    None => {
                        in_string.is_none()
                            && !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
                    }
                };
                if at_end {
                    flush_buffer(
                        &mut processed,
                        &mut buffer,
                        mode,
                        current_utf16_col,
                        &mut adjustments,
                    );
                    let start_suffix = utf16_count(&processed) as u32;
                    processed.push_str(");");
                    let end_suffix = utf16_count(&processed) as u32;
                    adjustments.push((current_utf16_col, start_suffix));
                    adjustments.push((current_utf16_col, end_suffix));
                    if term.is_some() {
                        // Consume the closing quote (masked tag markup).
                        char_idx += 1;
                        current_utf16_col += ch.len_utf16() as u32;
                        adjustments.push((current_utf16_col, end_suffix));
                    }
                    // The shorthand terminator (whitespace, `>`, `/`, …) is
                    // left for the HTML scanner to reprocess.
                    mode = Mode::Html;
                    continue;
                }
            }

            if mode != Mode::Html {
                if let Some(quote) = in_string {
                    if is_escaped {
                        is_escaped = false;
                    } else if ch == '\\' {
                        is_escaped = true;
                    } else if ch == quote {
                        in_string = None;
                    }
                    buffer.push(ch);
                    char_idx += 1;
                    current_utf16_col += ch.len_utf16() as u32;
                    continue;
                } else if ch == '\'' || ch == '"' {
                    in_string = Some(ch);
                    buffer.push(ch);
                    char_idx += 1;
                    current_utf16_col += ch.len_utf16() as u32;
                    continue;
                }
            }

            // In Verbatim mode, skip all content until @endverbatim
            if mode == Mode::Verbatim {
                let remaining = &line_chars[char_idx..];
                let rest_str: String = remaining.iter().collect();
                if rest_str.starts_with("@endverbatim") {
                    let directive_len = "@endverbatim".len();
                    char_idx += directive_len;
                    current_utf16_col += directive_len as u32;
                    mode = Mode::Html;
                } else {
                    // Skip char (it's inside the comment)
                    char_idx += 1;
                    current_utf16_col += ch.len_utf16() as u32;
                }
                continue;
            }

            let remaining = &line_chars[char_idx..];

            let mut match_len = 0;
            let mut replacement = String::new();
            let mut next_mode = mode;

            if mode == Mode::Html {
                if remaining.starts_with(&['{', '{']) {
                    let is_comment = remaining.starts_with(&['{', '{', '-', '-']);
                    let is_raw = remaining.starts_with(&['{', '{', '!', '!']);
                    replacement = if is_comment {
                        " /* ".to_string()
                    } else if is_raw {
                        " echo (".to_string()
                    } else {
                        " echo e(".to_string()
                    };
                    match_len = if is_comment || is_raw { 4 } else { 2 };
                    next_mode = Mode::Php;
                } else if remaining.starts_with(&['<', '?', 'p', 'h', 'p']) {
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
                } else if remaining.starts_with(&['@']) {
                    let rest_str: String = remaining[1..].iter().collect();
                    if let Some(directive) = match_directive(&rest_str) {
                        match_len = 1 + directive.len();
                        if directive == "php" {
                            let after_php = rest_str[3..].trim_start();
                            if !after_php.starts_with('(') {
                                in_php_directive_block = true;
                                next_mode = Mode::Php;
                                replacement = "".to_string();
                            } else {
                                replacement = format!(" {} ", translate_directive(directive));
                                next_mode = Mode::DirectiveArgs(";"); // Directive Args for @php(...)
                                paren_depth = 0;
                            }
                        } else if directive == "endphp" {
                            replacement = "".to_string();
                            next_mode = Mode::Html;
                        } else if directive == "verbatim" {
                            replacement = "".to_string();
                            next_mode = Mode::Verbatim;
                        } else if directive == "empty" {
                            // @empty with parens = if(empty(...)):, without parens = forelse separator
                            let after_dir: String = rest_str[directive.len()..].chars().collect();
                            let after_trimmed = after_dir.trim_start();
                            if after_trimmed.starts_with('(') {
                                // `translate_directive("empty")` opens an
                                // extra unmatched `(` (`if(empty`), so the
                                // directive's own closing paren needs a
                                // second `)` before the `:`.
                                replacement = format!(" {} ", translate_directive(directive));
                                next_mode = Mode::DirectiveArgs("):");
                                paren_depth = 0;
                            } else {
                                // forelse @empty (no args) → endforeach; if (false):
                                replacement = " endforeach; if (false): ".to_string();
                                next_mode = Mode::Html;
                            }
                        } else if matches!(directive, "session" | "context") {
                            replacement = " if (true) ".to_string();
                            next_mode = Mode::SkipArgs(": $value = '';");
                            paren_depth = 0;
                        } else if directive == "error" {
                            replacement = " if (true) ".to_string();
                            next_mode = Mode::SkipArgs(": $message = '';");
                            paren_depth = 0;
                        } else if matches!(
                            directive,
                            "auth" | "guest" | "production" | "env" | "once"
                        ) {
                            // These are conditional blocks: if args present, skip them;
                            // if no args, emit directly.
                            let after_dir: String = rest_str[directive.len()..].chars().collect();
                            let after_trimmed = after_dir.trim_start();
                            if after_trimmed.starts_with('(') {
                                replacement = " if (true) ".to_string();
                                next_mode = Mode::SkipArgs(":");
                                paren_depth = 0;
                            } else {
                                replacement = " if (true): ".to_string();
                                next_mode = Mode::Html;
                            }
                        } else if matches!(directive, "foreach" | "forelse") {
                            replacement = format!(" {} ", translate_directive(directive));
                            next_mode = Mode::DirectiveArgs(
                                ": /** @var object{index: int, iteration: int, remaining: int, count: int, first: bool, last: bool, even: bool, odd: bool, depth: int, parent: ?object} $loop */ $loop = (object)[];",
                            );
                            paren_depth = 0;
                        } else if matches!(
                            directive,
                            "if" | "elseif" | "for" | "while" | "switch" | "case"
                        ) {
                            replacement = format!(" {} ", translate_directive(directive));
                            next_mode = Mode::DirectiveArgs(":"); // Directive Args
                            paren_depth = 0;
                        } else if matches!(directive, "unless" | "isset") {
                            // `translate_directive` opens an extra unmatched
                            // `(` for both (`if(!` / `if(isset`), so the
                            // directive's own closing paren needs a second
                            // `)` before the `:`.
                            replacement = format!(" {} ", translate_directive(directive));
                            next_mode = Mode::DirectiveArgs("):");
                            paren_depth = 0;
                        } else if matches!(
                            directive,
                            "extends"
                                | "section"
                                | "yield"
                                | "include"
                                | "includeIf"
                                | "includeWhen"
                                | "includeUnless"
                                | "includeFirst"
                                | "push"
                                | "prepend"
                                | "component"
                                | "slot"
                                | "props"
                                | "aware"
                                | "fragment"
                                | "hasSection"
                                | "sectionMissing"
                                | "includeIsolated"
                                | "each"
                                | "pushIf"
                                | "pushOnce"
                                | "prependOnce"
                                | "hasstack"
                                | "method"
                                | "class"
                                | "style"
                                | "checked"
                                | "selected"
                                | "disabled"
                                | "readonly"
                                | "required"
                                | "stack"
                                | "json"
                                | "dump"
                        ) {
                            replacement = format!(" {} ", translate_directive(directive));
                            next_mode = Mode::DirectiveArgs(";"); // Directive Args for layout tags
                            paren_depth = 0;
                        } else if matches!(
                            directive,
                            "endif"
                                | "endforeach"
                                | "endfor"
                                | "endwhile"
                                | "endunless"
                                | "endisset"
                                | "endempty"
                                | "endswitch"
                                | "endforelse"
                                | "endsection"
                                | "endpush"
                                | "endprepend"
                                | "endcomponent"
                                | "endslot"
                                | "stop"
                                | "show"
                                | "append"
                                | "overwrite"
                                | "else"
                                | "default"
                                | "break"
                                | "endauth"
                                | "endguest"
                                | "endproduction"
                                | "endenv"
                                | "endsession"
                                | "endcontext"
                                | "enderror"
                                | "endonce"
                                | "endfragment"
                                | "endPushIf"
                                | "endPushOnce"
                                | "csrf"
                                | "parent"
                                | "continue"
                        ) {
                            replacement = format!(" {} ", translate_directive(directive));
                            next_mode = Mode::Html; // These don't take args and return to HTML mode immediately
                        } else if matches!(directive, "use" | "inject") {
                            // `@use(...)` / `@inject(...)` need their string
                            // argument(s) parsed into a real PHP construct, so
                            // the argument list is captured (not emitted
                            // verbatim) and transformed when it closes. Emit
                            // nothing inline until then.
                            let after_dir: String = rest_str[directive.len()..].chars().collect();
                            if after_dir.trim_start().starts_with('(') {
                                replacement = "".to_string();
                                next_mode = Mode::CaptureArgs(if directive == "use" {
                                    CapturedDirective::Use
                                } else {
                                    CapturedDirective::Inject
                                });
                                paren_depth = 0;
                            } else {
                                // Malformed (no argument list): mask and move on.
                                replacement = "".to_string();
                                next_mode = Mode::Html;
                            }
                        } else {
                            replacement = format!(" {}; ", translate_directive(directive));
                            next_mode = Mode::Php;
                        }
                    }
                } else if remaining.starts_with(&[':'])
                    && in_html_tag
                    && html_attr_string.is_none()
                    && (char_idx == 0 || line_chars[char_idx - 1].is_ascii_whitespace())
                    && remaining.get(1) != Some(&':')
                {
                    // A Blade component bound attribute at attribute
                    // position: `:name="$expr"`, `:name='$expr'`, or the
                    // `:$var` shorthand. The expression becomes a real PHP
                    // argument so its variables are seen; the rest of the
                    // tag stays masked. A leading `::` is an escaped literal
                    // colon and is left alone.
                    if remaining.get(1) == Some(&'$')
                        && remaining
                            .get(2)
                            .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_')
                    {
                        match_len = 1;
                        replacement = " blade_directive(".to_string();
                        next_mode = Mode::BoundAttr(None);
                    } else if let Some(open_len) = bound_attr_open_len(remaining) {
                        let quote = remaining[open_len - 1];
                        match_len = open_len;
                        replacement = " blade_directive(".to_string();
                        next_mode = Mode::BoundAttr(Some(quote));
                    }
                }
            } else if mode == Mode::Php {
                if remaining.starts_with(&['}', '}']) || remaining.starts_with(&['!', '!', '}']) {
                    let is_comment_end =
                        char_idx >= 2 && line_chars[char_idx - 2..].starts_with(&['-', '-']);
                    replacement = if is_comment_end {
                        " */ ".to_string()
                    } else {
                        "); ".to_string()
                    };
                    match_len = if remaining.starts_with(&['!', '!', '}']) {
                        3
                    } else {
                        2
                    };
                    next_mode = Mode::Html;
                } else if remaining.starts_with(&['@', 'e', 'n', 'd', 'p', 'h', 'p']) {
                    in_php_directive_block = false;
                    next_mode = Mode::Html;
                    match_len = 7;
                    replacement = "".to_string();
                }
            } else if let Mode::RawPhp(needs_semicolon) = mode {
                if remaining.starts_with(&['?', '>']) {
                    replacement = if needs_semicolon {
                        "; ".to_string()
                    } else {
                        "".to_string()
                    };
                    match_len = 2;
                    next_mode = Mode::Html;
                }
            } else if let Mode::DirectiveArgs(suffix) = mode {
                // In Directive Args, we wait for balanced parentheses
                if ch == '(' {
                    paren_depth += 1;
                } else if ch == ')' {
                    paren_depth -= 1;
                    if paren_depth <= 0 {
                        buffer.push(')');
                        char_idx += 1;
                        current_utf16_col += 1;
                        flush_buffer(
                            &mut processed,
                            &mut buffer,
                            mode,
                            current_utf16_col,
                            &mut adjustments,
                        );

                        let start_suffix = utf16_count(&processed) as u32;
                        processed.push_str(suffix);
                        let end_suffix = utf16_count(&processed) as u32;

                        adjustments.push((current_utf16_col, start_suffix));
                        adjustments.push((current_utf16_col, end_suffix));

                        mode = Mode::Html;
                        continue;
                    }
                }
            } else if let Mode::SkipArgs(suffix) = mode {
                // Consume balanced parens without outputting them
                if ch == '(' {
                    paren_depth += 1;
                } else if ch == ')' {
                    paren_depth -= 1;
                    if paren_depth <= 0 {
                        char_idx += 1;
                        current_utf16_col += 1;
                        // Discard buffer (args not output)
                        buffer.clear();

                        let start_suffix = utf16_count(&processed) as u32;
                        processed.push_str(suffix);
                        let end_suffix = utf16_count(&processed) as u32;

                        adjustments.push((current_utf16_col, start_suffix));
                        adjustments.push((current_utf16_col, end_suffix));

                        mode = Mode::Html;
                        continue;
                    }
                }
                // Don't output anything in SkipArgs - just advance
                char_idx += 1;
                current_utf16_col += ch.len_utf16() as u32;
                continue;
            } else if let Mode::CaptureArgs(kind) = mode {
                // Capture the argument text (in `buffer`, via the fall-through
                // push below) until the parens balance, then transform it.
                if ch == '(' {
                    paren_depth += 1;
                } else if ch == ')' {
                    paren_depth -= 1;
                    if paren_depth <= 0 {
                        char_idx += 1;
                        current_utf16_col += 1;
                        // `buffer` holds the argument text from the opening
                        // `(` up to (but not including) this closing `)`.
                        let raw = std::mem::take(&mut buffer);
                        let emitted = match kind {
                            CapturedDirective::Use => {
                                if let Some(stmt) = build_use_statement(&raw) {
                                    hoisted_uses.push(stmt);
                                }
                                // The import is hoisted; nothing inline.
                                String::new()
                            }
                            CapturedDirective::Inject => build_inject_statement(&raw),
                        };

                        let start_suffix = utf16_count(&processed) as u32;
                        processed.push_str(&emitted);
                        let end_suffix = utf16_count(&processed) as u32;

                        adjustments.push((current_utf16_col, start_suffix));
                        adjustments.push((current_utf16_col, end_suffix));

                        mode = Mode::Html;
                        in_string = None;
                        continue;
                    }
                }
            }

            if match_len > 0 || mode != next_mode {
                flush_buffer(
                    &mut processed,
                    &mut buffer,
                    mode,
                    current_utf16_col,
                    &mut adjustments,
                );

                if !replacement.is_empty() {
                    let start_php_col = utf16_count(&processed) as u32;
                    processed.push_str(&replacement);
                    let end_php_col = utf16_count(&processed) as u32;

                    // Boilerplate replacement: everything in the replacement
                    // (e.g. " echo e(") maps back to the START of the Blade
                    // tag.  This ensures that any semantic tokens Mago
                    // produces for the boilerplate (like the 'echo' keyword)
                    // have start == end in Blade space and are discarded.
                    adjustments.push((current_utf16_col, start_php_col));
                    adjustments.push((current_utf16_col, end_php_col));

                    char_idx += match_len;
                    current_utf16_col += match_len as u32;

                    // Anchor at the END of the Blade tag for subsequent content.
                    adjustments.push((current_utf16_col, end_php_col));
                } else {
                    // Empty replacement (e.g. @php)
                    adjustments.push((current_utf16_col, utf16_count(&processed) as u32));
                    char_idx += match_len;
                    current_utf16_col += match_len as u32;
                    adjustments.push((current_utf16_col, utf16_count(&processed) as u32));
                }

                mode = next_mode;
                continue;
            }

            // Track HTML tag / attribute-value state so bound attributes
            // are only recognized at attribute position (inside a tag, not
            // inside a quoted value). Colons in attribute values (e.g.
            // `href="mailto:x"`, `style="color:red"`) or in text between
            // tags (`10:30`) never satisfy `in_html_tag && !html_attr_string`.
            if mode == Mode::Html {
                match html_attr_string {
                    Some(q) if ch == q => html_attr_string = None,
                    Some(_) => {}
                    None => {
                        if ch == '<' {
                            // Enter a tag only when `<` begins an element
                            // (next char names a tag or is `/`), not on a
                            // stray `<` in text or a `< ` comparison.
                            let next = line_chars.get(char_idx + 1);
                            if next.is_none()
                                || next.is_some_and(|c| c.is_ascii_alphabetic() || *c == '/')
                            {
                                in_html_tag = true;
                            }
                        } else if ch == '>' {
                            in_html_tag = false;
                        } else if in_html_tag && (ch == '"' || ch == '\'') {
                            html_attr_string = Some(ch);
                        }
                    }
                }
            }

            buffer.push(ch);
            char_idx += 1;
            current_utf16_col += ch.len_utf16() as u32;
        }

        // A bound-attribute expression must not span lines. If its closing
        // quote never appeared on this line, close the `blade_directive(`
        // call here so the rest of the template is not corrupted.
        if let Mode::BoundAttr(_) = mode {
            flush_buffer(
                &mut processed,
                &mut buffer,
                mode,
                current_utf16_col,
                &mut adjustments,
            );
            processed.push_str(");");
            adjustments.push((current_utf16_col, utf16_count(&processed) as u32));
            mode = Mode::Html;
            in_string = None;
        }

        flush_buffer(
            &mut processed,
            &mut buffer,
            mode,
            current_utf16_col,
            &mut adjustments,
        );

        virtual_php.push_str(&processed);
        virtual_php.push('\n');
        adjustments.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        source_map.adjustments.push(adjustments);
    }

    // Close the wrapper function.
    virtual_php.push_str("}\n");

    // Emit collected `@use` imports as real top-level `use` statements.
    // They live after the wrapper function (and past every Blade line) so
    // they are valid PHP and do not shift the line-based source map.
    for stmt in &hoisted_uses {
        virtual_php.push_str(stmt);
        virtual_php.push('\n');
    }

    (virtual_php, source_map)
}

fn flush_buffer(
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

    if mode == Mode::Html {
        // HTML outside PHP/Directives — mask with spaces to maintain 1:1 utf-16 mapping.
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
        processed.push_str(buffer);
        adjustments.push((current_utf16_col, utf16_count(processed) as u32));
    }

    buffer.clear();
}

fn utf16_count(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Trim surrounding whitespace and quote characters, matching Blade's
/// compiler (`trim($x, " '\"")`).
fn trim_quotes_and_space(s: &str) -> &str {
    s.trim_matches(|c: char| c == ' ' || c == '\'' || c == '"')
}

/// Whether `s` is a valid PHP identifier (variable name without the `$`).
fn is_php_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Translate the captured argument text of an `@use(...)` directive into a
/// real top-level `use` statement, mirroring Blade's `compileUse`. `raw` is
/// everything from the opening `(` up to (not including) the closing `)`.
///
/// Handles the plain form (`'App\Models\Post'`), the inline alias
/// (`'App\Models\Post as Article'`), the two-argument alias
/// (`'App\Models\Post', 'Article'`), grouped imports
/// (`'App\Models\{Post, Comment}'`), and the `function`/`const` modifiers.
/// Returns `None` when no importable path can be parsed.
fn build_use_statement(raw: &str) -> Option<String> {
    // Blade strips all parens, then trims whitespace/quotes.
    let expression: String = raw.chars().filter(|c| *c != '(' && *c != ')').collect();
    let expression = trim_quotes_and_space(&expression);

    let (path_with_modifier, alias) = if expression.contains('{') {
        // Grouped import: the braces are the argument, no alias.
        (expression.to_string(), String::new())
    } else {
        let mut segments = expression.splitn(2, ',');
        let path = trim_quotes_and_space(segments.next().unwrap_or("")).to_string();
        let alias = match segments.next() {
            Some(a) => format!(" as {}", trim_quotes_and_space(a)),
            None => String::new(),
        };
        (path, alias)
    };

    // Split off a `function ` / `const ` modifier if present.
    let (modifier, path) = if let Some(rest) = path_with_modifier.strip_prefix("function ") {
        ("function ", rest)
    } else if let Some(rest) = path_with_modifier.strip_prefix("const ") {
        ("const ", rest)
    } else {
        ("", path_with_modifier.as_str())
    };
    let path = path.trim().trim_start_matches('\\');

    if path.is_empty() {
        return None;
    }

    Some(format!("use {modifier}{path}{alias};"))
}

/// Translate the captured argument text of an `@inject(...)` directive into
/// an inline `$var = app(service);` assignment, mirroring Blade's
/// `compileInject`. `raw` is everything from the opening `(` up to (not
/// including) the closing `)`. Returns an empty string when the argument
/// list has no valid variable name or service.
fn build_inject_statement(raw: &str) -> String {
    let stripped: String = raw.chars().filter(|c| *c != '(' && *c != ')').collect();
    let mut segments = stripped.splitn(2, ',');
    let variable = trim_quotes_and_space(segments.next().unwrap_or(""));
    // The service keeps its own quotes; only surrounding whitespace is trimmed.
    let service = segments.next().unwrap_or("").trim();

    if variable.is_empty() || !is_php_identifier(variable) || service.is_empty() {
        return String::new();
    }

    format!(" ${variable} = app({service}); ")
}

/// If `rem` (starting at a `:`) opens a `:name="` or `:name='` bound
/// attribute, return the length (in chars) of that opening span, up to and
/// including the opening quote. Returns `None` when the syntax does not
/// match, so the `:` is left as ordinary masked tag markup.
fn bound_attr_open_len(rem: &[char]) -> Option<usize> {
    // rem[0] is the ':'.
    let mut i = 1;
    let name_start = i;
    while i < rem.len() && (rem[i].is_ascii_alphanumeric() || matches!(rem[i], '_' | '-' | '.')) {
        i += 1;
    }
    if i == name_start {
        return None; // no attribute name after the colon
    }
    if rem.get(i) != Some(&'=') {
        return None;
    }
    i += 1;
    match rem.get(i) {
        Some('"') | Some('\'') => Some(i + 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `<?xml ... ?>` is never a PHP open tag regardless of
    /// `short_open_tag`; PHP special-cases it so XML declarations and
    /// feeds embedded in templates aren't misparsed as PHP.
    #[test]
    fn test_preprocess_xml_declaration_is_not_a_php_tag() {
        let content = "<?xml version=\"1.0\" ?>\n<users>\n    <user>{{ $user }}</user>\n</users>\n";
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("version"),
            "<?xml ...?> should be masked as HTML, not parsed as PHP: {}",
            php
        );
        assert!(
            php.contains("echo e( $user )"),
            "{{ $user }} after the XML declaration should still translate normally: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_directive_with_string_parens() {
        let content = "@if(str_contains($val, \")\"))\n    {{ $val }}\n@endif";
        let (php, _) = preprocess(content);
        // It should properly wait for the outer parenthesis to close
        assert!(
            php.contains(" if (str_contains($val, \")\")):"),
            "Failed to parse parens inside string: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_foreach_loop_variable() {
        let content = "@foreach($items as $item)\n{{ $loop->first }}\n@endforeach\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("$loop"),
            "should inject $loop variable: {}",
            php
        );
        assert!(
            php.contains("object{index: int"),
            "should have typed $loop: {}",
            php
        );
        // $loop should be declared before its usage
        let loop_decl = php.find("$loop = (object)[];").unwrap();
        let loop_use = php.rfind("$loop").unwrap();
        assert!(
            loop_use > loop_decl,
            "$loop usage after declaration: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_errors_bag_visible_inside_template_function() {
        let content = "{{ $errors->has('name') }}";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("function __blade_template() { global $errors, $__env;"),
            "$errors/$__env must be pulled into the wrapper function's scope: {}",
            php
        );
    }

    /// Inline attribute directives (`@class`, `@style`, `@checked`,
    /// `@selected`, `@disabled`, `@readonly`, `@required`) must consume
    /// their own argument list and return to HTML mode, not fall into the
    /// generic directive branch (which leaves everything after them
    /// parsed as PHP for the rest of the template).
    #[test]
    fn test_preprocess_attribute_directives_return_to_html() {
        let content = r#"<div @class(['a', 'b' => $cond]) id="x"></div>"#;
        let (php, _) = preprocess(content);
        // HTML content is masked with spaces (it is not meant to be parsed
        // as PHP), so the literal `id="x"` markup must NOT survive as raw
        // PHP source after the directive — that was the bug: the
        // generic-directive fallback left the parser in PHP mode for the
        // rest of the template, so `id="x"></div>` leaked through
        // unmasked and caused cascading syntax errors.
        assert!(
            !php.contains(r#"id="x""#),
            "content after @class(...) should be masked as HTML, not left as raw PHP: {}",
            php
        );
        assert!(
            php.contains("blade_directive (['a', 'b' => $cond]);"),
            "unexpected @class(...) translation: {}",
            php
        );
    }

    /// `@stack('name')` (render a named stack) must consume its own
    /// argument list and return to HTML mode, like `@yield`/`@section`,
    /// instead of falling into the generic directive branch.
    #[test]
    fn test_preprocess_stack_directive_returns_to_html() {
        let content = r#"<div>@stack('scripts')</div><p>after</p>"#;
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("after"),
            "content after @stack(...) should be masked as HTML, not left as raw PHP: {}",
            php
        );
        assert!(
            php.contains("blade_directive ('scripts');"),
            "unexpected @stack(...) translation: {}",
            php
        );
    }

    /// `@json($var)` must consume its argument as a real expression so a
    /// variable used only inside it is not silently invisible to the
    /// forward walker (it previously fell outside `match_directive`
    /// entirely, so `$var` in `@json($var)` was never emitted as PHP and
    /// the variable was reported as unused).
    #[test]
    fn test_preprocess_json_directive_consumes_argument() {
        let content = r#"<script>window.foo = @json($value);</script><p>after</p>"#;
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("after"),
            "content after @json(...) should be masked as HTML, not left as raw PHP: {}",
            php
        );
        assert!(
            php.contains("blade_directive ($value);"),
            "unexpected @json(...) translation: {}",
            php
        );
    }

    /// `@dump($var)` must likewise consume its argument as a real
    /// expression, for the same reason as `@json` above.
    #[test]
    fn test_preprocess_dump_directive_consumes_argument() {
        let content = r#"<div>@dump($value)</div><p>after</p>"#;
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("after"),
            "content after @dump(...) should be masked as HTML, not left as raw PHP: {}",
            php
        );
        assert!(
            php.contains("blade_directive ($value);"),
            "unexpected @dump(...) translation: {}",
            php
        );
    }

    /// A bound attribute on a component tag (`:src="$image"`) must emit
    /// its expression as real PHP so the variable is seen by the forward
    /// walker (otherwise a variable used only there is a false-positive
    /// "unused variable"). The surrounding tag markup stays masked.
    #[test]
    fn test_preprocess_bound_attribute_emits_expression() {
        let content = r#"<x-img.size :src="$image" alt="x" />"#;
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_directive($image);"),
            "bound attribute expression should be emitted as PHP: {}",
            php
        );
        // The tag name and other attribute markup must not leak as raw PHP.
        assert!(
            !php.contains("x-img.size"),
            "tag markup should stay masked: {}",
            php
        );
        assert!(
            !php.contains(r#"alt="x""#),
            "unbound attribute markup should stay masked: {}",
            php
        );
    }

    /// Package tag namespaces (`<livewire:...>`) and method-call
    /// expressions inside the binding must work the same way.
    #[test]
    fn test_preprocess_bound_attribute_livewire_and_method_call() {
        let content = r#"<livewire:edit-channel :key="$item->id" />"#;
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_directive($item->id);"),
            "method-call expression in a bound attribute should be emitted: {}",
            php
        );
        // The `:` inside the `livewire:edit-channel` tag name is part of
        // the name, not an attribute, so it must not open a directive call.
        assert!(
            !php.contains("blade_directive(edit-channel"),
            "namespace colon in the tag name must not be treated as a binding: {}",
            php
        );
    }

    /// The `:$var` shorthand expands to a bound `var` attribute whose
    /// expression is `$var`.
    #[test]
    fn test_preprocess_bound_attribute_shorthand() {
        let content = r#"<x-alert :$message />"#;
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_directive($message);"),
            "`:$var` shorthand should emit the variable as PHP: {}",
            php
        );
    }

    /// A bound attribute whose value contains a PHP string literal (with
    /// the opposite quote) must be captured whole, not truncated at the
    /// inner quote.
    #[test]
    fn test_preprocess_bound_attribute_with_inner_string() {
        let content = r#"<x-btn :class="$active ? 'on' : 'off'" />"#;
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_directive($active ? 'on' : 'off');"),
            "inner string literals should be preserved in the expression: {}",
            php
        );
    }

    /// Colons that are not at attribute position must never be treated as
    /// bindings: inside an attribute value (`mailto:`), in text between
    /// tags (`10:30`), or as an escaped literal colon (`::class`).
    #[test]
    fn test_preprocess_bound_attribute_does_not_misfire_on_value_colons() {
        let content =
            "<a href=\"mailto:x@example.com\">10:30</a>\n<x-c ::class=\"literal\" :real=\"$v\" />";
        let (php, _) = preprocess(content);
        // The only binding here is `:real="$v"`.
        assert!(
            php.contains("blade_directive($v);"),
            "the real binding should still be emitted: {}",
            php
        );
        // The prologue declares `function blade_directive(...)` once, so a
        // single binding yields two occurrences of `blade_directive(`.
        assert_eq!(
            php.matches("blade_directive(").count(),
            2,
            "no spurious bindings from value/text/escaped colons: {}",
            php
        );
        // `mailto:` and the escaped `::class` literal must stay masked.
        assert!(
            !php.contains("mailto"),
            "attr value must stay masked: {}",
            php
        );
        assert!(
            !php.contains("literal"),
            "escaped `::` attribute must stay masked: {}",
            php
        );
    }

    /// A `:name="..."` written outside any tag (in text) must not be
    /// treated as a binding.
    #[test]
    fn test_preprocess_bound_attribute_ignored_outside_tag() {
        let content = r#"<p>ratio :w="16" here</p>"#;
        let (php, _) = preprocess(content);
        // Only the prologue's `function blade_directive(...)` declaration
        // should remain; no binding call is emitted for a colon in text.
        assert_eq!(
            php.matches("blade_directive(").count(),
            1,
            "a colon in text (outside a tag span) is not a binding: {}",
            php
        );
    }

    /// A bound attribute split across lines from its tag opener must still
    /// be recognized (tags span multiple lines in real templates).
    #[test]
    fn test_preprocess_bound_attribute_multiline_tag() {
        let content = "<x-img.size\n    :src=\"$image\"\n/>";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_directive($image);"),
            "binding on a continuation line should be recognized: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_forelse_loop_variable() {
        let content = "@forelse($items as $item)\n{{ $loop->index }}\n@empty\n@endforelse\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("$loop = (object)[];"),
            "forelse should also inject $loop: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_echo_with_string_braces() {
        let content = "{{ \"}} \" }}";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo e( \"}} \" );"),
            "Failed to parse braces inside string: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_foreach() {
        let content = r#"@php
/**
 * @var \App\Models\AuthorCollection $users
 */
@endphp

@foreach($users->active()->byName() as $user)
    <p>{{ $user->name }}</p>
@endforeach
"#;
        let (php, _) = preprocess(content);
        for (i, line) in php.lines().enumerate() {
            eprintln!("{:2}: {}", i, line);
        }
        assert!(php.contains("$user->name"));
    }

    #[test]
    fn test_preprocess_forelse() {
        let content = r#"@forelse($users as $user)
    <p>{{ $user->name }}</p>
@empty
    <p>No users</p>
@endforelse
"#;
        let (php, _) = preprocess(content);
        for (i, line) in php.lines().enumerate() {
            eprintln!("{:2}: {}", i, line);
        }
        assert!(php.contains("foreach"), "should contain foreach: {}", php);
        assert!(
            php.contains("endforeach"),
            "should contain endforeach: {}",
            php
        );
        assert!(
            php.contains("if (false):"),
            "should contain if (false): {}",
            php
        );
        assert!(php.contains("endif;"), "should contain endif: {}", php);
    }

    #[test]
    fn test_preprocess_session_directive() {
        let content = "@session('key')\n    <p>{{ $value }}</p>\n@endsession\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true)"),
            "should contain if (true): {}",
            php
        );
        assert!(
            php.contains("$value = '';"),
            "should inject $value: {}",
            php
        );
        assert!(php.contains("endif;"), "should contain endif: {}", php);
    }

    #[test]
    fn test_preprocess_verbatim() {
        let content =
            "@verbatim\n    {{ $name }}\n    @if(true)\n@endverbatim\n<p>{{ $real }}</p>\n";
        let (php, _) = preprocess(content);
        // The {{ $name }} inside verbatim should NOT produce echo
        assert!(
            !php.contains("$name"),
            "verbatim content should be skipped: {}",
            php
        );
        // The {{ $real }} after @endverbatim should work normally
        assert!(
            php.contains("$real"),
            "content after endverbatim should work: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_verbatim_with_comment_syntax() {
        // Verbatim blocks may contain */ which would break PHP block comments
        let content =
            "@verbatim\n    {{ /* js comment */ value }}\n@endverbatim\n<p>{{ $after }}</p>\n";
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("js comment"),
            "verbatim content should be skipped: {}",
            php
        );
        assert!(
            php.contains("$after"),
            "content after endverbatim should work: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_error_directive() {
        let content = "@error('email')\n    <p>{{ $message }}</p>\n@enderror\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true)"),
            "should contain if (true): {}",
            php
        );
        assert!(
            php.contains("$message = '';"),
            "should inject $message: {}",
            php
        );
        assert!(php.contains("endif;"), "should contain endif: {}", php);
    }

    #[test]
    fn test_preprocess_context_directive() {
        let content = "@context('key')\n    <p>{{ $value }}</p>\n@endcontext\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true)"),
            "should contain if (true): {}",
            php
        );
        assert!(
            php.contains("$value = '';"),
            "should inject $value: {}",
            php
        );
        assert!(php.contains("endif;"), "should contain endif: {}", php);
    }

    #[test]
    fn test_preprocess_prologue_declares_view_directive() {
        let (php, _) = preprocess("<p>hello</p>");
        assert!(
            php.contains("function blade_view_directive"),
            "prologue should declare blade_view_directive: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_multiline_directive() {
        let content = "@include('vendor.fbRemarket', [\n    'facebook_pixel_id' => Config::get('services.facebook.pixel_id'),\n])\n\n@include('vendor.googleRemarket')";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_view_directive"),
            "@include should produce blade_view_directive call: {}",
            php
        );

        let content2 = "{{\n    $var\n}}";
        let (php2, _) = preprocess(content2);
        assert!(
            php2.contains("$var"),
            "Multiline echo should preserve variable: {}",
            php2
        );
    }

    #[test]
    fn test_preprocess_stub_directives() {
        // @csrf should produce a comment (no-args directive)
        let content = "@csrf\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("/* @csrf */"),
            "@csrf should become a comment: {}",
            php
        );

        // @auth without args should produce if (true):
        let content = "@auth\n<p>logged in</p>\n@endauth\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true):"),
            "@auth should produce if (true):: {}",
            php
        );
        assert!(
            php.contains("endif;"),
            "@endauth should produce endif;: {}",
            php
        );

        // @auth with args should also produce if (true):
        let content = "@auth('admin')\n<p>admin</p>\n@endauth\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true)"),
            "@auth('admin') should produce if (true): {}",
            php
        );
        assert!(
            php.contains("endif;"),
            "@endauth should produce endif;: {}",
            php
        );

        // @guest without args
        let content = "@guest\n<p>guest</p>\n@endguest\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true):"),
            "@guest should produce if (true):: {}",
            php
        );
        assert!(
            php.contains("endif;"),
            "@endguest should produce endif;: {}",
            php
        );

        // @production (never takes args)
        let content = "@production\n<p>prod</p>\n@endproduction\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true):"),
            "@production should produce if (true):: {}",
            php
        );
        assert!(
            php.contains("endif;"),
            "@endproduction should produce endif;: {}",
            php
        );

        // @env with args
        let content = "@env('local')\n<p>local</p>\n@endenv\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true)"),
            "@env should produce if (true): {}",
            php
        );
        assert!(
            php.contains("endif;"),
            "@endenv should produce endif;: {}",
            php
        );

        // @once without args
        let content = "@once\n<script>app.js</script>\n@endonce\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (true):"),
            "@once should produce if (true):: {}",
            php
        );
        assert!(
            php.contains("endif;"),
            "@endonce should produce endif;: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_raw_php_tag_preserves_at_prefixed_string() {
        // A raw <?php ... ?> block (not @php/@endphp) containing a string
        // literal that starts with '@' (e.g. a JSON-LD '@context' key) must
        // not be misread as a Blade directive.
        let content = "@php\n@endphp\n<?php\n$schema = ['@context' => 'x'];\n?>\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("'@context' => 'x'"),
            "raw PHP tag content should pass through verbatim: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_raw_php_tag_short_echo() {
        let content = "<p><?= $value ?></p>";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo  $value ;"),
            "<?= ?> should translate to an echo statement: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_switch_case_with_class_constant() {
        let content = "@switch($x)\n    @case (App\\Enums\\E::A)\n        {{ 1 }}\n        @break\n@endswitch\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("case  (App\\Enums\\E::A):"),
            "@case should preserve its argument and emit a trailing colon: {}",
            php
        );
        assert!(php.contains("break;"), "@break should emit break;: {}", php);
    }

    #[test]
    fn test_preprocess_session_value_accessible() {
        // $value should be accessible inside @session block
        let content = "@session('status')\n{{ $value }}\n@endsession\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("$value = '';"),
            "should declare $value: {}",
            php
        );
        // The $value echo should appear after the declaration
        let val_decl = php.find("$value = '';").unwrap();
        // Find last occurrence of $value (the echo usage)
        let val_echo = php.rfind("$value").unwrap();
        assert!(
            val_echo > val_decl,
            "$value usage should come after declaration: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_error_message_accessible() {
        // $message should be accessible inside @error block
        let content = "@error('email')\n{{ $message }}\n@enderror\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("$message = '';"),
            "should declare $message: {}",
            php
        );
        let msg_decl = php.find("$message = '';").unwrap();
        let msg_echo = php.rfind("$message").unwrap();
        assert!(
            msg_echo > msg_decl,
            "$message usage should come after declaration: {}",
            php
        );
    }

    /// `@unless`/`@isset`/`@empty(...)` translate to `if(!`/`if(isset`/
    /// `if(empty` respectively — an extra, unmatched opening paren on top
    /// of the directive's own argument parens — so the directive needs a
    /// second closing paren before the trailing `:`, or the next PHP
    /// parser sees `unexpected token ':', expected ')'` and the rest of
    /// the template is corrupted.
    #[test]
    fn test_preprocess_unless_isset_empty_close_extra_paren() {
        let (unless_php, _) = preprocess("@unless($cond)\nx\n@endunless\n<p>after</p>");
        assert!(
            unless_php.contains("if(! ($cond)):"),
            "@unless should close both the synthetic and the argument paren: {}",
            unless_php
        );

        let (isset_php, _) = preprocess("@isset($var)\nx\n@endisset\n<p>after</p>");
        assert!(
            isset_php.contains("if(isset ($var)):"),
            "@isset should close both the synthetic and the argument paren: {}",
            isset_php
        );

        let (empty_php, _) = preprocess("@empty($var)\nx\n@endempty\n<p>after</p>");
        assert!(
            empty_php.contains("if(empty ($var)):"),
            "@empty(...) should close both the synthetic and the argument paren: {}",
            empty_php
        );
    }

    /// `@use('App\Models\Post')` must become a real top-level `use` import
    /// (hoisted out of the wrapper function), and must not leave the parser
    /// in PHP mode corrupting the rest of the template.
    #[test]
    fn test_preprocess_use_directive_emits_import() {
        let content = "@use('App\\Models\\Post')\n<p>after</p>";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("use App\\Models\\Post;"),
            "@use should emit a real use import: {}",
            php
        );
        // The import is hoisted after the wrapper function's closing brace,
        // so it must appear after `}` (top-level, not inside the function).
        let brace = php.rfind('}').unwrap();
        let import = php.find("use App\\Models\\Post;").unwrap();
        assert!(
            import > brace,
            "the use import must be hoisted to the top level: {}",
            php
        );
        // Content after @use must stay masked as HTML, not leak as raw PHP.
        assert!(
            !php.contains("after"),
            "content after @use(...) should be masked as HTML: {}",
            php
        );
    }

    /// The inline-alias form `@use('App\Models\Post as Article')` keeps the
    /// alias.
    #[test]
    fn test_preprocess_use_directive_inline_alias() {
        let content = "@use('App\\Models\\Post as Article')\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("use App\\Models\\Post as Article;"),
            "@use with an inline `as` should preserve the alias: {}",
            php
        );
    }

    /// The two-argument alias form `@use('App\Models\Post', 'Article')`
    /// produces the same aliased import.
    #[test]
    fn test_preprocess_use_directive_second_arg_alias() {
        let content = "@use('App\\Models\\Post', 'Article')\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("use App\\Models\\Post as Article;"),
            "@use with a second alias argument should preserve the alias: {}",
            php
        );
    }

    /// The `function`/`const` modifiers are carried through to the import.
    #[test]
    fn test_preprocess_use_directive_function_modifier() {
        let content = "@use('function App\\Support\\helper')\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("use function App\\Support\\helper;"),
            "@use with a function modifier should emit `use function`: {}",
            php
        );
    }

    /// `@inject('metrics', 'App\Services\Metrics')` becomes an inline
    /// `$metrics = app(...)` assignment so the injected variable is defined
    /// and typed, and does not corrupt the rest of the template.
    #[test]
    fn test_preprocess_inject_directive_emits_assignment() {
        let content = "@inject('metrics', 'App\\Services\\Metrics')\n<p>after</p>";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("$metrics = app('App\\Services\\Metrics');"),
            "@inject should emit an inline app() assignment: {}",
            php
        );
        // The assignment is inline (inside the wrapper function), so it must
        // come before the wrapper function's closing brace.
        let brace = php.rfind('}').unwrap();
        let assign = php.find("$metrics = app(").unwrap();
        assert!(
            assign < brace,
            "the inject assignment must stay inside the wrapper function: {}",
            php
        );
        assert!(
            !php.contains("after"),
            "content after @inject(...) should be masked as HTML: {}",
            php
        );
    }

    /// `@inject` with a `::class` service expression is preserved verbatim
    /// (Blade keeps the second argument unquoted-trimmed).
    #[test]
    fn test_preprocess_inject_directive_class_constant_service() {
        let content = "@inject('repo', App\\Repo::class)\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("$repo = app(App\\Repo::class);"),
            "@inject should preserve a ::class service expression: {}",
            php
        );
    }
}
