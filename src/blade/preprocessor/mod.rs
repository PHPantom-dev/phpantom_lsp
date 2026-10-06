use super::TemplateKind;
use super::directives::CustomDirectives;
use super::source_map::BladeSourceMap;

mod capture_args;
mod component_call;
mod directive;
mod echo;
mod php_island;
mod prologue;
mod resolver;
mod shared;
mod tag;
#[cfg(test)]
mod tests;

pub use component_call::ARGUMENT_VAR_PREFIX;
pub use resolver::{ComponentBinding, ComponentParameter, ComponentResolver, ComponentTarget};

use component_call::OpenComponentCall;
use echo::EchoCloses;
use shared::{LineOut, Lowering, flush_buffer, utf16_count};
use tag::{BoundAttr, HtmlPos};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Html,
    /// A Blade echo escaped with `@` (`@{{ ... }}` or `@{!! ... !!}`).
    /// Laravel removes the leading `@` and leaves the whole echo for the
    /// frontend template engine, so none of its contents are PHP. The `bool`
    /// is true for a raw echo, whose terminator is `!!}` instead of `}}`.
    EscapedEcho(bool),
    /// PHP expression/statement content scanned for the `}}` / `!!}` echo
    /// terminators and `@endphp`. The `bool` is true when the mode was
    /// entered through a raw `{!! … !!}` echo, whose emitted `echo` has no
    /// `e(` wrapper and so must be closed with a bare `;` instead of `);`.
    Php(bool),
    /// A raw `<?php` / `<?=` / `<?` tag embedded directly in the template
    /// (i.e. not via `@php`/`@endphp`). Content is passed through verbatim
    /// with no directive/echo scanning, and the mode ends at `?>`. The
    /// `bool` tracks whether the opening tag was a short-echo tag (`<?=`),
    /// which needs a trailing `;` injected before the closing `?>`.
    RawPhp(bool),
    DirectiveArgs(&'static str),
    SkipArgs(&'static str),
    Verbatim,
    /// The body of a `{{-- ... --}}` comment, emitted as a PHP `/* ... */`
    /// block. Comment text is neither PHP nor Blade, so nothing in it but the
    /// `--}}` terminator carries meaning: an apostrophe must not start a
    /// string literal (the scanner would hunt for a matching closing quote), a
    /// commented-out `}}`/`!!}` or an `@endphp` in prose must not end the
    /// comment, and a literal `*/` in the text must not close the emitted
    /// block. Any of those desyncs the rest of the file.
    Comment,
    /// The expression of a Blade component bound attribute
    /// (`:name="$expr"` or the `:$var` shorthand). The expression is
    /// emitted verbatim as a real PHP argument to
    /// `blade_bound_attr_directive(...)` so the forward walker sees the
    /// variables it uses; the surrounding tag markup stays masked.
    /// `Some(quote)` is the delimiting quote of a `:name="..."` value;
    /// `None` is the shorthand `:$var`, which ends at the first character
    /// that cannot be part of the variable name.
    BoundAttr(Option<char>),
    /// The parenthesised argument list of an `@use(...)` or `@inject(...)`
    /// directive. Unlike `DirectiveArgs`, the argument text is captured and
    /// transformed (rather than emitted verbatim) so the correct real PHP
    /// construct can be produced when the list closes.
    CaptureArgs(CapturedDirective),
}

/// Which directive is having its argument list captured by
/// [`Mode::CaptureArgs`]. Each has a different real-PHP translation:
/// `@use` becomes a top-level `use` import (hoisted out of the wrapper
/// function) and `@inject` becomes an inline `$var = app(service);`
/// assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapturedDirective {
    Use,
    Inject,
}

pub fn preprocess(content: &str) -> (String, BladeSourceMap) {
    preprocess_with_vars(
        content,
        &[],
        TemplateKind::View,
        None,
        None,
        &CustomDirectives::default(),
    )
}

/// Like [`preprocess`], but seeds the template's scope with externally
/// inferred variables (name without `$`, docblock type string).  Each
/// variable is declared in the top-level prologue with a `@var` docblock
/// and pulled into the wrapper function via `global`, the same mechanism
/// that makes `$errors`/`$__env` visible to every consumer (forward
/// walker, docblock backward scan, undefined-variable diagnostics).
///
/// Every variable the template does not assign itself is declared in the
/// prologue, following the priority chain in [`super::signature`]: the
/// template's own signature docblock wins, then `@props`/`@aware`, then the
/// variables Blade injects into a component body, then the externally
/// resolved variables the caller passes in (a backing class's members and
/// the layouts the template extends ahead of call-site inference, in the
/// order given).  A name declared by a higher source is not re-declared by
/// a lower one.
///
/// A signature-declared name is deliberately left out: its docblock stays
/// in the template body, where the forward walker reads it and carries the
/// type over the rest of the file.  Re-declaring it here would put a second
/// (and, for a `@props` default, a *wrong*) type in front of the author's.
///
/// `this_class` is the fully qualified name of the class a template renders
/// with bound to `$this` (Livewire hands its view the component instance).
/// `$this` cannot arrive through the declaration channel above, since PHP
/// allows neither `$this = …` nor `global $this`, so the body is wrapped in
/// a method of a synthesized subclass of that class instead of in a plain
/// function.
///
/// `components` resolves the `<x-…>` and `<livewire:…>` tags the template
/// renders to the classes behind them, so that `$component` after a tag
/// carries that class's members and the tag's attributes are checked as
/// the arguments the framework passes them as.  Without one (or for a tag
/// it cannot answer for) the tag degrades to a comment.
///
/// `custom_directives` are the ones the project's service providers
/// registered with `Blade::directive()` / `Blade::if()`.  A directive in
/// that set lowers to a marker call keeping its argument as real PHP,
/// instead of degrading to the comment an unrecognised `@name` becomes.
pub fn preprocess_with_vars(
    content: &str,
    injected_vars: &[(String, String)],
    kind: TemplateKind,
    this_class: Option<&str>,
    components: Option<&dyn ComponentResolver>,
    custom_directives: &CustomDirectives,
) -> (String, BladeSourceMap) {
    let mut virtual_php = String::with_capacity(content.len() + 512);
    let mut source_map = BladeSourceMap::default();

    let uses_insert_at = prologue::emit(
        content,
        injected_vars,
        kind,
        this_class,
        &mut virtual_php,
        &mut source_map,
    );

    // `@use` imports cannot be emitted inline: the template body is wrapped
    // in `function __blade_template()`, and PHP `use` imports are only valid
    // at the top level. They are collected here and spliced into the
    // prologue as real top-level `use` statements once the scan is done.
    let mut hoisted_uses: Vec<String> = Vec::new();

    let mut in_php_directive_block = false;
    let mut mode = Mode::Html;
    let mut paren_depth = 0;
    let mut in_string: Option<char> = None;
    let mut is_escaped = false;
    // A `/* ... */` comment can span lines, so it is tracked like
    // `in_string` above; a `//`/`#` comment always ends at the newline, so
    // it is tracked per-line instead (declared inside the line loop below).
    let mut in_block_comment = false;
    let mut html = HtmlPos {
        in_tag: false,
        attr_string: None,
    };
    // Text captured by `Mode::CaptureArgs` from lines before the current
    // one. A captured argument list (e.g. a multi-line `@props([...])`
    // array) can span several lines, but the per-line `buffer` below is
    // reset every iteration of the outer loop, so each line's contribution
    // is appended here (instead of being flushed into `processed`) until
    // the closing paren is reached and the whole span is transformed as
    // one unit.
    let mut capture_buffer = String::new();
    let mut bound_attr = BoundAttr {
        suffix: ");",
        multiline: false,
    };
    // The component call the surrounding tag opened, if any; see
    // `OpenComponentCall`.
    let mut open_call: Option<OpenComponentCall> = None;

    let lines: Vec<&str> = content.lines().collect();

    let echo_closes = EchoCloses {
        escaped: lines.iter().rposition(|l| l.contains("}}")),
        raw: lines.iter().rposition(|l| l.contains("!!}")),
    };
    // Whether the echo currently open in `Mode::Php` has no terminator
    // anywhere ahead of it. Blade compiles an unpaired opener as literal
    // text, but masking it would break completion inside an echo that is
    // simply not finished being typed yet, so the expression is kept and
    // closed at end of line instead: one line degrades rather than the
    // whole rest of the template being swallowed as PHP. Line-scoped:
    // reset at the top of each line, since an echo it applies to never
    // survives the line that opened it.
    let mut echo_closes_at_eol;

    for (line_idx, line) in lines.iter().enumerate() {
        let mut processed = String::new();
        let mut adjustments = vec![(0, 0)]; // (blade_utf16_col, php_utf16_col)

        let mut current_utf16_col = 0;
        let line_chars: Vec<char> = line.chars().collect();
        let mut buffer = String::new();
        // The lines a construct opening on this one can run into.
        let following = &lines[line_idx + 1..];

        echo_closes_at_eol = false;
        let mut in_line_comment = false;

        if mode == Mode::Html && in_php_directive_block {
            mode = Mode::Php(false);
        }

        let mut char_idx = 0;
        while char_idx < line_chars.len() {
            let ch = line_chars[char_idx];

            // Close a bound-attribute expression when its terminator is
            // reached. This must run before the generic string tracking
            // below, otherwise the closing `"` of a `:name="..."` value
            // would be mistaken for the start of a PHP string literal.
            if let Mode::BoundAttr(term) = mode
                && tag::close_bound_attr(
                    term,
                    ch,
                    in_string,
                    &bound_attr,
                    LineOut {
                        processed: &mut processed,
                        buffer: &mut buffer,
                        adjustments: &mut adjustments,
                        char_idx: &mut char_idx,
                        current_utf16_col: &mut current_utf16_col,
                    },
                )
            {
                // The shorthand terminator (whitespace, `>`, `/`, …) is
                // left for the HTML scanner to reprocess.
                mode = Mode::Html;
                continue;
            }

            if !matches!(
                mode,
                Mode::Html | Mode::EscapedEcho(_) | Mode::Comment | Mode::Verbatim
            ) {
                if in_line_comment {
                    buffer.push(ch);
                    char_idx += 1;
                    current_utf16_col += ch.len_utf16() as u32;
                    continue;
                } else if in_block_comment {
                    buffer.push(ch);
                    char_idx += 1;
                    current_utf16_col += ch.len_utf16() as u32;
                    if ch == '*' && line_chars.get(char_idx) == Some(&'/') {
                        buffer.push('/');
                        char_idx += 1;
                        current_utf16_col += 1;
                        in_block_comment = false;
                    }
                    continue;
                } else if let Some(quote) = in_string {
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
                } else if ch == '/' && line_chars.get(char_idx + 1) == Some(&'*') {
                    in_block_comment = true;
                    buffer.push(ch);
                    char_idx += 1;
                    current_utf16_col += 1;
                    continue;
                } else if (ch == '/' && line_chars.get(char_idx + 1) == Some(&'/'))
                    || (ch == '#' && line_chars.get(char_idx + 1) != Some(&'['))
                {
                    // A bare `#` starts a shell-style comment, but `#[` opens a
                    // PHP attribute instead.
                    in_line_comment = true;
                    buffer.push(ch);
                    char_idx += 1;
                    current_utf16_col += 1;
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
                    char_idx += 1;
                    current_utf16_col += ch.len_utf16() as u32;
                }
                continue;
            }

            let remaining = &line_chars[char_idx..];

            let mut lowering = Lowering::keep(mode);

            if mode == Mode::Html {
                if let Some(matched) =
                    echo::open(remaining, line_idx, &echo_closes, &mut echo_closes_at_eol)
                {
                    lowering = matched;
                } else if let Some(matched) = php_island::open(remaining) {
                    lowering = matched;
                } else if let Some(matched) =
                    tag::open(remaining, &mut html, following, components, &mut open_call)
                {
                    lowering = matched;
                } else if let Some(matched) = tag::close(remaining, &mut html, &mut open_call) {
                    lowering = matched;
                } else if let Some(matched) =
                    echo::open_escaped(remaining, line_idx, &echo_closes, &mut echo_closes_at_eol)
                {
                    lowering = matched;
                } else if let Some(matched) = (char_idx == 0
                    || !is_word_char(line_chars[char_idx - 1]))
                .then(|| {
                    // Blade anchors directives with `\B`, so the `@` in
                    // `support@foreach.example` is text, not a loop.
                    directive::open(
                        remaining,
                        custom_directives,
                        &mut paren_depth,
                        &mut in_php_directive_block,
                    )
                })
                .flatten()
                {
                    lowering = matched;
                } else if let Some(matched) = tag::bound_attr(
                    remaining,
                    &line_chars,
                    char_idx,
                    &html,
                    following,
                    &mut open_call,
                    &mut bound_attr,
                ) {
                    lowering = matched;
                }
            } else if let Mode::EscapedEcho(raw) = mode {
                lowering = echo::close_escaped(raw, remaining, custom_directives);
            } else if mode == Mode::Comment {
                lowering = echo::close_comment(remaining, &line_chars, char_idx);
            } else if let Mode::Php(raw_echo) = mode {
                lowering = echo::close_php(
                    raw_echo,
                    remaining,
                    &mut in_php_directive_block,
                    custom_directives,
                );
            } else if let Mode::RawPhp(needs_semicolon) = mode {
                lowering = php_island::close(needs_semicolon, remaining);
            } else if let Mode::DirectiveArgs(suffix) = mode {
                if directive::consume_args(
                    suffix,
                    ch,
                    mode,
                    &mut paren_depth,
                    LineOut {
                        processed: &mut processed,
                        buffer: &mut buffer,
                        adjustments: &mut adjustments,
                        char_idx: &mut char_idx,
                        current_utf16_col: &mut current_utf16_col,
                    },
                ) {
                    mode = Mode::Html;
                    continue;
                }
            } else if let Mode::SkipArgs(suffix) = mode {
                if directive::skip_args(
                    suffix,
                    ch,
                    &mut paren_depth,
                    LineOut {
                        processed: &mut processed,
                        buffer: &mut buffer,
                        adjustments: &mut adjustments,
                        char_idx: &mut char_idx,
                        current_utf16_col: &mut current_utf16_col,
                    },
                ) {
                    mode = Mode::Html;
                }
                continue;
            } else if let Mode::CaptureArgs(kind) = mode
                && capture_args::consume(
                    kind,
                    ch,
                    &mut paren_depth,
                    &mut capture_buffer,
                    &mut hoisted_uses,
                    LineOut {
                        processed: &mut processed,
                        buffer: &mut buffer,
                        adjustments: &mut adjustments,
                        char_idx: &mut char_idx,
                        current_utf16_col: &mut current_utf16_col,
                    },
                )
            {
                mode = Mode::Html;
                in_string = None;
                continue;
            }

            let Lowering {
                match_len,
                replacement,
                next_mode,
            } = lowering;

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

            if mode == Mode::Html {
                tag::track(ch, &line_chars, char_idx, &mut html);
            }

            buffer.push(ch);
            char_idx += 1;
            current_utf16_col += ch.len_utf16() as u32;
        }

        // An echo opener with nothing left in the file that could close it
        // is literal text to Blade, but masking it would break completion
        // inside an echo that is simply not finished being typed yet. Keep
        // the expression and close it at end of line instead, so at most
        // one line degrades rather than every later line being emitted as
        // PHP and the wrapper's closing brace landing inside the unclosed
        // echo.
        if let Mode::Php(raw_echo) = mode
            && echo_closes_at_eol
        {
            flush_buffer(
                &mut processed,
                &mut buffer,
                mode,
                current_utf16_col,
                &mut adjustments,
            );
            processed.push_str(if raw_echo { "; " } else { "); " });
            adjustments.push((current_utf16_col, utf16_count(&processed) as u32));
            mode = Mode::Html;
            in_string = None;
        }

        // The same for an `@`-escaped echo: a `@{{` the file never closes is
        // not an escape to Blade at all, just literal text. Masking on past
        // this line would swallow the `@endif`/`@endforeach` of every block
        // it sits in and leave the emitted PHP unbalanced, which reports the
        // whole template as a syntax error while the escape is still being
        // typed.
        if matches!(mode, Mode::EscapedEcho(_)) && echo_closes_at_eol {
            mode = Mode::Html;
        }

        // A bound-attribute expression whose closing quote is on a later
        // line (what a formatter produces for a long array or argument
        // list) stays open: this line's PHP is flushed as-is and the next
        // line continues the same `blade_bound_attr_directive(` call.
        // Cutting it off here would truncate the expression mid-syntax.
        //
        // When the closing quote never appears at all the attribute is
        // malformed, and the call is closed off so only the attribute
        // itself is lost rather than the rest of the template.
        if let Mode::BoundAttr(_) = mode {
            flush_buffer(
                &mut processed,
                &mut buffer,
                mode,
                current_utf16_col,
                &mut adjustments,
            );
            if !bound_attr.multiline {
                processed.push_str(bound_attr.suffix);
                adjustments.push((current_utf16_col, utf16_count(&processed) as u32));
                mode = Mode::Html;
                in_string = None;
            }
        }

        if let Mode::CaptureArgs(_) = mode {
            // The argument list is still open at end of line: defer this
            // line's text instead of flushing it into `processed`, which
            // would leak a raw fragment into the virtual PHP before the
            // closing paren transforms the whole span as one unit.
            capture_buffer.push_str(&buffer);
            capture_buffer.push('\n');
            buffer.clear();
        } else {
            flush_buffer(
                &mut processed,
                &mut buffer,
                mode,
                current_utf16_col,
                &mut adjustments,
            );
        }

        virtual_php.push_str(&processed);
        virtual_php.push('\n');
        adjustments.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        source_map.adjustments.push(adjustments);
    }

    // An unterminated `{{--` leaves the emitted `/*` open, which would
    // swallow the wrapper's closing brace and make the whole file
    // unparseable. Close it so only the comment itself is lost.
    if mode == Mode::Comment {
        virtual_php.push_str(" */\n");
    }

    // Likewise for a multi-line bound attribute whose closing quote turned
    // out to be unreachable: leaving `blade_bound_attr_directive(` open
    // would swallow the wrapper's closing brace.
    if let Mode::BoundAttr(_) = mode {
        virtual_php.push_str(bound_attr.suffix);
        virtual_php.push('\n');
    }

    // And for a component tag whose `>` the template never reaches.
    if let Some(call) = open_call.take() {
        virtual_php.push_str(&call.close());
        virtual_php.push('\n');
    }

    // Close the wrapper function, and the class holding it when the body
    // was wrapped in a method.
    virtual_php.push_str(if this_class.is_some() { "} }\n" } else { "}\n" });

    // Splice the collected `@use` imports into the prologue as real
    // top-level `use` statements, and grow the prologue height by the
    // lines they add so every Blade position still maps correctly.
    if !hoisted_uses.is_empty() {
        let mut block = String::new();
        for stmt in &hoisted_uses {
            block.push_str(stmt);
            block.push('\n');
        }
        source_map.prologue_lines += hoisted_uses.len() as u32;
        virtual_php.insert_str(uses_insert_at, &block);
    }

    (virtual_php, source_map)
}

/// A `\w` character in the byte-oriented sense Blade's compiler regexes use.
fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}
