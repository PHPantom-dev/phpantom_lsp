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
    // appended after the main loop.
    virtual_php.push_str("function __blade_template() {\n");

    let mut in_php_directive_block = false;
    let mut mode = Mode::Html;
    let mut paren_depth = 0;
    let mut in_string: Option<char> = None;
    let mut is_escaped = false;

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
                                replacement = format!(" {} ", translate_directive(directive));
                                next_mode = Mode::DirectiveArgs(":");
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
                            "if" | "elseif"
                                | "for"
                                | "while"
                                | "switch"
                                | "unless"
                                | "isset"
                                | "case"
                        ) {
                            replacement = format!(" {} ", translate_directive(directive));
                            next_mode = Mode::DirectiveArgs(":"); // Directive Args
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
                        } else {
                            replacement = format!(" {}; ", translate_directive(directive));
                            next_mode = Mode::Php;
                        }
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

            buffer.push(ch);
            char_idx += 1;
            current_utf16_col += ch.len_utf16() as u32;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
