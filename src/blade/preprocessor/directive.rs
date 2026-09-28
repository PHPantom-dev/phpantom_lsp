use super::shared::{LineOut, Lowering, closes_args, flush_buffer};
use super::{CapturedDirective, Mode};
use crate::blade::directives::{
    CUSTOM_MARKER, CustomDirectives, CustomForm, match_directive, translate_directive,
};

/// An `@name` directive opening at the cursor.
pub(super) fn open(
    remaining: &[char],
    custom_directives: &CustomDirectives,
    paren_depth: &mut i32,
    in_php_directive_block: &mut bool,
) -> Option<Lowering> {
    let mut match_len = 0;
    let mut replacement = String::new();
    let mut next_mode = Mode::Html;

    if remaining.starts_with(&['@']) {
        let rest_str: String = remaining[1..].iter().collect();
        if let Some(directive) = match_directive(&rest_str) {
            match_len = 1 + directive.len();
            if directive == "php" {
                let after_php = rest_str[3..].trim_start();
                if !after_php.starts_with('(') {
                    *in_php_directive_block = true;
                    next_mode = Mode::Php(false);
                    replacement = "".to_string();
                } else {
                    replacement = format!(" {} ", translate_directive(directive));
                    next_mode = Mode::DirectiveArgs(";");
                    *paren_depth = 0;
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
                    *paren_depth = 0;
                } else {
                    replacement = " endforeach; if (false): ".to_string();
                    next_mode = Mode::Html;
                }
            } else if matches!(directive, "session" | "context") {
                replacement = " if (true) ".to_string();
                next_mode = Mode::SkipArgs(": $value = '';");
                *paren_depth = 0;
            } else if directive == "error" {
                replacement = " if (true) ".to_string();
                next_mode = Mode::SkipArgs(": $message = '';");
                *paren_depth = 0;
            } else if matches!(directive, "auth" | "guest" | "production" | "env" | "once") {
                // These are conditional blocks: if args present, skip them;
                // if no args, emit directly.
                let after_dir: String = rest_str[directive.len()..].chars().collect();
                let after_trimmed = after_dir.trim_start();
                if after_trimmed.starts_with('(') {
                    replacement = " if (true) ".to_string();
                    next_mode = Mode::SkipArgs(":");
                    *paren_depth = 0;
                } else {
                    replacement = " if (true): ".to_string();
                    next_mode = Mode::Html;
                }
            } else if matches!(directive, "foreach" | "forelse") {
                replacement = format!(" {} ", translate_directive(directive));
                next_mode = Mode::DirectiveArgs(
                    ": /** @var object{index: int, iteration: int, remaining: int, count: int, first: bool, last: bool, even: bool, odd: bool, depth: int, parent: ?object{index: int, iteration: int, remaining: int, count: int, first: bool, last: bool, even: bool, odd: bool, depth: int, parent: ?object}} $loop */ $loop = (object)[];",
                );
                *paren_depth = 0;
            } else if matches!(
                directive,
                "if" | "elseif" | "for" | "while" | "switch" | "case"
            ) {
                replacement = format!(" {} ", translate_directive(directive));
                next_mode = Mode::DirectiveArgs(":");
                *paren_depth = 0;
            } else if matches!(
                directive,
                "unless"
                    | "isset"
                    | "can"
                    | "cannot"
                    | "canany"
                    | "elsecan"
                    | "elsecannot"
                    | "elsecanany"
                    | "hasStack"
                    | "hasSection"
                    | "sectionMissing"
            ) {
                // `translate_directive` opens an extra unmatched
                // `(` for all of these (`if(!` / `if(isset` /
                // `if (blade_directive` / `elseif (blade_directive`),
                // so the directive's own closing paren needs a
                // second `)` before the `:`.
                replacement = format!(" {} ", translate_directive(directive));
                next_mode = Mode::DirectiveArgs("):");
                *paren_depth = 0;
            } else if matches!(
                directive,
                "extends"
                    | "extendsFirst"
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
                    | "componentFirst"
                    | "slot"
                    | "props"
                    | "aware"
                    | "fragment"
                    | "includeIsolated"
                    | "each"
                    | "pushIf"
                    | "pushOnce"
                    | "prependOnce"
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
                    | "unset"
                    | "choice"
                    | "js"
                    | "dd"
            ) {
                replacement = format!(" {} ", translate_directive(directive));
                next_mode = Mode::DirectiveArgs(";");
                *paren_depth = 0;
            } else if directive == "lang" {
                // `@lang` is either a bare block opener paired
                // with `@endlang` (translation buffering that
                // always runs, so it has nothing to type-check)
                // or `@lang('key')` / `@lang(['key' => ...])`,
                // a one-shot call whose argument is a real
                // expression.
                let after_dir: String = rest_str[directive.len()..].chars().collect();
                if after_dir.trim_start().starts_with('(') {
                    replacement = format!(" {} ", translate_directive(directive));
                    next_mode = Mode::DirectiveArgs(";");
                    *paren_depth = 0;
                } else {
                    replacement = "".to_string();
                    next_mode = Mode::Html;
                }
            } else if matches!(directive, "vite" | "fonts") {
                // Both take an optional argument list (Laravel
                // defaults it to `()` when omitted), so a bare
                // `@vite` / `@fonts` must not enter
                // `DirectiveArgs`, which would otherwise consume
                // the rest of the template hunting for a closing
                // paren that was never opened.
                let after_dir: String = rest_str[directive.len()..].chars().collect();
                if after_dir.trim_start().starts_with('(') {
                    replacement = format!(" {} ", translate_directive(directive));
                    next_mode = Mode::DirectiveArgs(";");
                    *paren_depth = 0;
                } else {
                    replacement = "".to_string();
                    next_mode = Mode::Html;
                }
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
                    | "endcomponentFirst"
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
                    | "endPrependOnce"
                    | "csrf"
                    | "parent"
                    | "continue"
                    | "endcan"
                    | "endcannot"
                    | "endcanany"
                    | "endlang"
                    | "viteReactRefresh"
            ) {
                replacement = format!(" {} ", translate_directive(directive));
                next_mode = Mode::Html; // These don't take args and return to HTML mode immediately
            } else if matches!(directive, "use" | "inject") {
                // `@use(...)` / `@inject(...)` need their
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
                    *paren_depth = 0;
                } else {
                    // Malformed (no argument list): mask and move on.
                    replacement = "".to_string();
                    next_mode = Mode::Html;
                }
            } else {
                replacement = format!(" {}; ", translate_directive(directive));
                next_mode = Mode::Php(false);
            }
        } else if let Some((name, form)) = custom_directives.match_directive(&rest_str) {
            // A directive one of the project's service providers
            // registered. Blade's own compiler checks its custom
            // table *before* its built-in directives, but a
            // registration shadowing a core name would break the
            // block structure of every template that writes it
            // (and of Blade's own compiled output), so the core
            // table wins here.
            //
            // The handler is a callback returning arbitrary PHP,
            // so only the argument list is reproduced: it stays
            // real PHP that gets type-checked, passed to a marker
            // that stands in for whatever the handler emits. An
            // argument list is optional — Blade hands the handler
            // an empty expression when there is none — so a bare
            // name must not enter `DirectiveArgs`, which would
            // hunt the rest of the template for a closing paren
            // that was never opened.
            match_len = 1 + name.len();
            let has_args = rest_str[name.len()..].trim_start().starts_with('(');
            match form {
                CustomForm::End => {
                    replacement = " endif; ".to_string();
                    next_mode = Mode::Html;
                }
                CustomForm::Open | CustomForm::Else => {
                    let keyword = if form == CustomForm::Open {
                        "if"
                    } else {
                        "elseif"
                    };
                    if has_args {
                        // The marker's own `(` is left open for
                        // the directive's argument list to close,
                        // so the suffix closes both it and the
                        // condition.
                        replacement = format!(" {keyword} ({CUSTOM_MARKER} ");
                        next_mode = Mode::DirectiveArgs("):");
                        *paren_depth = 0;
                    } else {
                        replacement = format!(" {keyword} ({CUSTOM_MARKER}()): ");
                        next_mode = Mode::Html;
                    }
                }
                CustomForm::Statement => {
                    if has_args {
                        replacement = format!(" {CUSTOM_MARKER} ");
                        next_mode = Mode::DirectiveArgs(";");
                        *paren_depth = 0;
                    } else {
                        replacement = format!(" {CUSTOM_MARKER}(); ");
                        next_mode = Mode::Html;
                    }
                }
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

/// Emit a directive's argument list verbatim until its parens balance,
/// reporting whether the closing paren was reached.
pub(super) fn consume_args(
    suffix: &'static str,
    ch: char,
    mode: Mode,
    paren_depth: &mut i32,
    mut out: LineOut<'_>,
) -> bool {
    // In Directive Args, we wait for balanced parentheses
    if closes_args(ch, paren_depth) {
        out.buffer.push(')');
        *out.char_idx += 1;
        *out.current_utf16_col += 1;
        flush_buffer(
            out.processed,
            out.buffer,
            mode,
            *out.current_utf16_col,
            out.adjustments,
        );

        out.emit_suffix(suffix);

        return true;
    }
    false
}

/// Consume a directive's argument list without emitting it, reporting
/// whether the closing paren was reached. The cursor is advanced past the
/// current character either way, so the caller always resumes the scan.
pub(super) fn skip_args(
    suffix: &'static str,
    ch: char,
    paren_depth: &mut i32,
    mut out: LineOut<'_>,
) -> bool {
    // Consume balanced parens without outputting them
    if closes_args(ch, paren_depth) {
        *out.char_idx += 1;
        *out.current_utf16_col += 1;
        out.buffer.clear();

        out.emit_suffix(suffix);

        return true;
    }
    *out.char_idx += 1;
    *out.current_utf16_col += ch.len_utf16() as u32;
    false
}
