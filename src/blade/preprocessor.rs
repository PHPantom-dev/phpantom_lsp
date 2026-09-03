use super::TemplateKind;
use super::directives::{
    CUSTOM_MARKER, CustomDirectives, CustomForm, match_directive, translate_directive,
};
use super::source_map::BladeSourceMap;

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

/// Resolves the class a component tag names, so the preprocessor can bind
/// `$component` to it and check the tag's attributes against the call the
/// framework makes with them.
///
/// The preprocessor never reaches into the project index itself: it runs
/// on every keystroke and from the parallel index workers, where building
/// the Blade discovery index would put a workspace walk on the edit path.
/// The caller passes in whatever index it already has, and a tag it cannot
/// answer for degrades to a comment.
pub trait ComponentResolver {
    /// The class an `<x-…>` tag names: the component class behind a
    /// class-based component, or `Illuminate\View\AnonymousComponent` for
    /// a tag that names a template with no class of its own.
    fn x_component(&self, tag: &str) -> Option<ComponentTarget>;

    /// The class a `<livewire:…>` tag names.
    fn livewire_component(&self, name: &str) -> Option<ComponentTarget>;
}

/// The class a component tag names, and what the tag's attributes are to
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentTarget {
    /// Fully qualified class name, without a leading `\`.
    pub fqn: String,
    pub binding: ComponentBinding,
}

/// How a resolved component tag reaches its class.
///
/// Laravel partitions a tag's attributes by the signature it is about to
/// call: the ones naming a parameter are its arguments and the rest go to
/// the component's attribute bag (`ComponentTagCompiler::partitionDataAndAttributes`).
/// Reproducing that split is what lets the attributes be checked as the
/// arguments they are without an attribute meant for the bag being read as
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentBinding {
    /// `$component = new \Fqn(heading: 'Latest', post: $post);` — a Blade
    /// component's attributes are its constructor's arguments.
    Construct(Vec<ComponentParameter>),
    /// `$component = new \Fqn(); $component->mount(post: $post);` — a
    /// Livewire component is built by the container and handed its
    /// attributes through `mount()`.
    Mount(Vec<ComponentParameter>),
    /// `/** @var \Fqn $component */ $component = null;` — the class is
    /// known but the tag's attributes are arguments to nothing: an
    /// anonymous component's attributes are its *view's* variables rather
    /// than a signature's.
    Declare,
}

/// One parameter a component tag's attributes can fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentParameter {
    /// The parameter name (no `$`), which an attribute has to camel-case
    /// to in order to fill it.
    pub name: String,
    /// What the call passes when no attribute fills this parameter:
    /// `null` for a nullable one and `resolve(\Foo::class)` for one the
    /// container can build, which is how Laravel itself fills a
    /// constructor the tag left incomplete.  `None` when the parameter
    /// has a default (the call just omits it) or when nothing stands in
    /// for it, which is the case Laravel fails on and the missing-argument
    /// diagnostic is right to report.
    pub fallback: Option<String>,
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

/// The variables Blade puts in a component view's scope on top of the data
/// its caller passes: (name without `$`, docblock type, initialiser).
///
/// No caller passes these — Blade injects them when it renders the
/// component — so no signature or `@props` list can be expected to declare
/// them.
const COMPONENT_VARS: [(&str, &str, &str); 3] = [
    (
        "attributes",
        "\\Illuminate\\View\\ComponentAttributeBag",
        "new \\Illuminate\\View\\ComponentAttributeBag()",
    ),
    (
        "slot",
        "\\Illuminate\\View\\ComponentSlot",
        "new \\Illuminate\\View\\ComponentSlot()",
    ),
    ("componentName", "string", "''"),
];

/// A type string that is safe to place inside a one-line `/** @var … */`
/// docblock, or `mixed` when it is not.
///
/// Inferred types are rendered from expressions in caller files, so they
/// can carry arbitrary text: a literal-string type keeps its source form,
/// and PHP allows a real line break inside a quoted string. A line break
/// would add a prologue line the source map has to account for, and a
/// `*/` would close the docblock early and spill the rest into code.
/// Neither is worth reproducing faithfully, so such a type degrades to
/// `mixed` and the variable is still declared.
fn docblock_safe_type(type_string: &str) -> &str {
    let usable = !type_string.trim().is_empty()
        && !type_string.contains(['\n', '\r'])
        && !type_string.contains("*/");
    if usable { type_string } else { "mixed" }
}

/// Whether `name` (without the `$`) is something PHP can bind as a
/// variable.
///
/// A component tag's attributes become the template's variables, but an
/// attribute name is HTML, not PHP: `wire:model.live`, `@click` and
/// `x-on:keydown` are all legal there.  Blade hands the data to
/// `extract()`, which silently skips any key that is not a valid variable
/// name, so those attributes are reachable only through `$attributes`.
/// Declaring one anyway would emit `$wire:model.live = null;` into the
/// prologue and break the whole template with a syntax error.
fn is_php_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || !first.is_ascii())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || !ch.is_ascii())
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

    let signature = super::signature::extract(content);
    // (name without `$`, the PHP that declares it), in priority order.
    let mut declared: Vec<(String, String)> = Vec::new();
    let mut declare = |name: &str, decl: String| {
        if !is_php_variable_name(name)
            || signature.declares(name)
            || declared.iter().any(|(existing, _)| existing == name)
        {
            return;
        }
        declared.push((name.to_string(), decl));
    };

    // `@props`/`@aware` entries. A default value types its prop directly
    // (the expression is emitted verbatim, so anything the type engine can
    // resolve works); an entry without one is a *required* prop, whose
    // value the caller supplies, so it is declared `mixed` rather than
    // being invented as `null`.
    let entries = super::signature::extract_props(content)
        .into_iter()
        .chain(super::signature::extract_aware(content))
        .flatten();
    for entry in entries {
        let decl = match &entry.default {
            Some(default) => format!("${} = {};\n", entry.name, default),
            None => format!(
                "/** @var mixed ${name} */\n${name} = null;\n",
                name = entry.name
            ),
        };
        declare(&entry.name, decl);
    }

    if kind == TemplateKind::Component {
        for &(name, type_name, init) in &COMPONENT_VARS {
            declare(
                name,
                format!("/** @var {type_name} ${name} */\n${name} = {init};\n"),
            );
        }
    }

    for (name, type_string) in injected_vars {
        let type_string = docblock_safe_type(type_string);
        declare(
            name,
            format!("/** @var {type_string} ${name} */\n${name} = null;\n"),
        );
    }

    // ── Prologue ──
    virtual_php.push_str("<?php if (!function_exists('blade_directive')) { function blade_directive(...$args) {} function blade_bound_attr_directive(...$args) {} function blade_view_directive(...$args) {} function blade_each_directive(...$args) {} function blade_can_directive(...$args): bool { return true; } function blade_section_directive(...$args): bool { return true; } function blade_stack_directive(...$args): bool { return true; } function blade_push_if_directive(...$args) {} function blade_custom_directive(...$args): bool { return true; } }\n");
    // Where hoisted `@use` imports are spliced in once the whole
    // template has been scanned: still in the prologue, so they precede
    // every name they import (name resolution runs in source order and
    // an import written after a use of the name does not apply to it).
    let uses_insert_at = virtual_php.len();
    virtual_php.push_str("/** @var \\Illuminate\\Support\\ViewErrorBag $errors */\n");
    virtual_php.push_str("$errors = new \\Illuminate\\Support\\ViewErrorBag();\n");
    virtual_php.push_str("/** @var \\Illuminate\\View\\Factory $__env */\n");
    virtual_php.push_str("$__env = new \\Illuminate\\View\\Factory();\n");
    for (_, decl) in &declared {
        virtual_php.push_str(decl);
    }

    // Wrap the template body in a function so that diagnostic
    // collectors (which only analyse function/method bodies) treat
    // the Blade content as analysable code.  The closing brace is
    // appended after the main loop.  `$errors`/`$__env` (and every
    // declared variable) are assigned in the outer scope above, so
    // pull them in with `global` — otherwise every use of them inside
    // the wrapped function is a false-positive "undefined variable".
    //
    // A template that renders with a component instance bound gets a
    // method of a subclass of that component instead, so `$this` resolves
    // off the component the way it does in any other method body.  The
    // subclass is abstract: it exists only to carry the body, and a
    // concrete one would be reported for every method its parent leaves
    // abstract.
    if let Some(fqn) = this_class {
        virtual_php.push_str("abstract class ");
        virtual_php.push_str(&super::scope_class_name(fqn));
        virtual_php.push_str(" extends \\");
        virtual_php.push_str(fqn.trim_matches('\\'));
        virtual_php.push_str(" { public ");
    }
    virtual_php.push_str("function ");
    virtual_php.push_str(super::WRAPPER_FUNCTION);
    virtual_php.push_str("() { global $errors, $__env");
    for (name, _) in &declared {
        virtual_php.push_str(", $");
        virtual_php.push_str(name);
    }
    virtual_php.push_str(";\n");
    // Derive the prologue height from what was actually emitted rather
    // than assuming a line count per injected variable.  Every Blade
    // position is offset by this number, so a type string that carried
    // an unexpected line break would shift the whole file.
    source_map.prologue_lines = virtual_php.matches('\n').count() as u32;

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
    // Whether the HTML scanner is currently between the `<` and `>` of a
    // tag, and (when inside a tag) whether it is inside a quoted attribute
    // value. Both persist across lines so multi-line tags are tracked
    // correctly. They gate recognition of `:name="$expr"` bound
    // attributes, which are only valid at attribute position inside a tag.
    let mut in_html_tag = false;
    let mut html_attr_string: Option<char> = None;
    // Text captured by `Mode::CaptureArgs` from lines before the current
    // one. A captured argument list (e.g. a multi-line `@props([...])`
    // array) can span several lines, but the per-line `buffer` below is
    // reset every iteration of the outer loop, so each line's contribution
    // is appended here (instead of being flushed into `processed`) until
    // the closing paren is reached and the whole span is transformed as
    // one unit.
    let mut capture_buffer = String::new();
    // Whether the bound attribute currently open in `Mode::BoundAttr` has
    // its closing quote on a later line, so the expression must stay open
    // at end of line instead of being closed off. Set when the attribute
    // opens; see `bound_attr_spans_lines`.
    let mut bound_attr_multiline = false;
    // What closes the expression currently open in `Mode::BoundAttr`:
    // `);` for the `blade_bound_attr_directive(` call an ordinary bound
    // attribute becomes, and `;` for one that is an argument of the
    // surrounding tag's component call and is bound to a variable for it.
    let mut bound_attr_suffix = ");";
    // The component call the surrounding tag opened, if any; see
    // `OpenComponentCall`.
    let mut open_call: Option<OpenComponentCall> = None;

    let lines: Vec<&str> = content.lines().collect();

    // The last line holding each echo terminator, computed once so an echo
    // opener can ask "is there a terminator anywhere after me?" without
    // rescanning the rest of the file per opener (an opener with none
    // would otherwise cost O(file) each, O(file²) across a file of them).
    let last_escaped_echo_close = lines.iter().rposition(|l| l.contains("}}"));
    let last_raw_echo_close = lines.iter().rposition(|l| l.contains("!!}"));
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

        echo_closes_at_eol = false;

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
                    processed.push_str(bound_attr_suffix);
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

            if !matches!(mode, Mode::Html | Mode::EscapedEcho(_) | Mode::Comment) {
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
                if remaining.starts_with(&['{', '{'])
                    && !remaining[1..].starts_with(&['{', '!', '!'])
                {
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
                        echo_closes_at_eol = !contains_seq(&remaining[2..], &['}', '}'])
                            && last_escaped_echo_close.is_none_or(|last| last <= line_idx);
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
                    echo_closes_at_eol = !contains_seq(&remaining[3..], &['!', '!', '}'])
                        && last_raw_echo_close.is_none_or(|last| last <= line_idx);
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
                } else if html_attr_string.is_none()
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
                    let (text, call) =
                        tag.emit(target, &remaining[tag.len..], &lines[line_idx + 1..]);
                    replacement = text;
                    if call.is_some() {
                        open_call = call;
                    }
                    // The tag's `<` went into the replacement instead of
                    // reaching the tag-state tracker below, so mark the
                    // tag open by hand — otherwise `:attr="$expr"` inside
                    // a component tag would not be at attribute position.
                    in_html_tag = true;
                } else if open_call.is_some()
                    && html_attr_string.is_none()
                    && (remaining.starts_with(&['>']) || remaining.starts_with(&['/', '>']))
                {
                    // The tag closes, which is where the call it makes is
                    // emitted: everything between the tag's name and here
                    // is markup that became statements.
                    match_len = if remaining[0] == '/' { 2 } else { 1 };
                    replacement = open_call.take().expect("call is open").close();
                    in_html_tag = false;
                } else if remaining.starts_with(&['@', '{', '{'])
                    || remaining.starts_with(&['@', '{', '!', '!'])
                {
                    // The `@` escapes the complete Blade echo for a frontend
                    // template engine. Mask everything through its closing
                    // delimiter rather than exposing the expression as PHP.
                    let raw = remaining[2] == '!';
                    match_len = if raw { 4 } else { 3 };
                    next_mode = Mode::EscapedEcho(raw);
                } else if remaining.starts_with(&['@']) {
                    let rest_str: String = remaining[1..].iter().collect();
                    if let Some(directive) = match_directive(&rest_str) {
                        match_len = 1 + directive.len();
                        if directive == "php" {
                            let after_php = rest_str[3..].trim_start();
                            if !after_php.starts_with('(') {
                                in_php_directive_block = true;
                                next_mode = Mode::Php(false);
                                replacement = "".to_string();
                            } else {
                                replacement = format!(" {} ", translate_directive(directive));
                                next_mode = Mode::DirectiveArgs(";");
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
                            next_mode = Mode::DirectiveArgs(":");
                            paren_depth = 0;
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
                            paren_depth = 0;
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
                            paren_depth = 0;
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
                                paren_depth = 0;
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
                                paren_depth = 0;
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
                                paren_depth = 0;
                            } else {
                                // Malformed (no argument list): mask and move on.
                                replacement = "".to_string();
                                next_mode = Mode::Html;
                            }
                        } else {
                            replacement = format!(" {}; ", translate_directive(directive));
                            next_mode = Mode::Php(false);
                        }
                    } else if let Some((name, form)) = custom_directives.match_directive(&rest_str)
                    {
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
                                    paren_depth = 0;
                                } else {
                                    replacement = format!(" {keyword} ({CUSTOM_MARKER}()): ");
                                    next_mode = Mode::Html;
                                }
                            }
                            CustomForm::Statement => {
                                if has_args {
                                    replacement = format!(" {CUSTOM_MARKER} ");
                                    next_mode = Mode::DirectiveArgs(";");
                                    paren_depth = 0;
                                } else {
                                    replacement = format!(" {CUSTOM_MARKER}(); ");
                                    next_mode = Mode::Html;
                                }
                            }
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
                        let name = super::component_tags::camel_case_attr_name(
                            &remaining[name_span].iter().collect::<String>(),
                        );
                        let argument = open_call
                            .as_mut()
                            .and_then(|call| call.take(&name))
                            .map(|variable| format!(" {variable} = "));
                        let (prefix, suffix) = match &argument {
                            Some(prefix) => (prefix.as_str(), ";"),
                            None => (" blade_bound_attr_directive(", ");"),
                        };
                        replacement = prefix.to_string();
                        bound_attr_suffix = suffix;

                        if shorthand {
                            match_len = 1;
                            next_mode = Mode::BoundAttr(None);
                            bound_attr_multiline = false;
                        } else {
                            let open_len = bound_attr_open_len(remaining).expect("name parsed");
                            let quote = remaining[open_len - 1];
                            match_len = open_len;
                            next_mode = Mode::BoundAttr(Some(quote));
                            bound_attr_multiline = bound_attr_spans_lines(
                                quote,
                                &remaining[open_len..],
                                &lines[line_idx + 1..],
                            );
                        }
                    }
                }
            } else if let Mode::EscapedEcho(raw) = mode {
                let closes_echo = if raw {
                    remaining.starts_with(&['!', '!', '}'])
                } else {
                    remaining.starts_with(&['}', '}'])
                };
                if closes_echo {
                    match_len = if raw { 3 } else { 2 };
                    next_mode = Mode::Html;
                }
            } else if mode == Mode::Comment {
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
            } else if let Mode::Php(raw_echo) = mode {
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
                        // `capture_buffer` holds any prior lines of this
                        // argument list; `buffer` holds the current line's
                        // text from the opening `(` (or line start) up to
                        // (but not including) this closing `)`. Together
                        // they are the argument text from the opening `(`
                        // to the closing `)`.
                        let mut raw = std::mem::take(&mut capture_buffer);
                        raw.push_str(&buffer);
                        buffer.clear();
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
            if !bound_attr_multiline {
                processed.push_str(bound_attr_suffix);
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
        virtual_php.push_str(bound_attr_suffix);
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

/// The prefixes a component tag is written under: `<x-…>` names a Blade
/// component and `<livewire:…>` a Livewire one, each resolved through its
/// own index.
const TAG_PREFIXES: [&str; 2] = ["x-", "livewire:"];

/// A `<x-…>` / `<livewire:…>` tag opening (or its closing counterpart)
/// found at the current scan position.
struct ComponentTag {
    /// Characters from the `<` up to (not including) the first character
    /// after the tag name — the attribute list is left to the HTML
    /// scanner.
    len: usize,
    /// Which index resolves [`Self::name`].
    prefix: &'static str,
    /// The name as written, without the prefix (`alert`, `forms.input`,
    /// `pkg::calendar`, `counter`).
    name: String,
    closing: bool,
}

impl ComponentTag {
    /// The class this tag names, or `None` for a closing tag, one Blade
    /// claims for itself, or one no index answers for.
    fn resolve(&self, components: Option<&dyn ComponentResolver>) -> Option<ComponentTarget> {
        if self.closing || self.is_reserved() {
            return None;
        }
        let resolver = components?;
        match self.prefix {
            "livewire:" => resolver.livewire_component(&self.name),
            _ => resolver.x_component(&self.name),
        }
    }

    /// The PHP this tag becomes, and the call its attributes fill.
    ///
    /// A tag whose class resolves binds `$component` to it, the variable
    /// Blade's own compiled output uses, so `$component->` inside the tag
    /// body carries the component's members.  When the class also has a
    /// signature the attributes are arguments to, the whole call is
    /// emitted where the tag *closes*, since everything between the tag's
    /// name and its `>` is markup the scanner turns into statements — a
    /// `{{ }}` echo, a directive, a bound attribute the attribute bag
    /// takes — and none of those can sit in an argument list.  A bound
    /// attribute that is an argument is bound to a variable where it is
    /// written, so hovering the expression still lands on the template's
    /// own text, and the call passes that variable.
    ///
    /// Everything else (a closing tag, a component no index knows,
    /// `<x-dynamic-component>`, a `<x-slot>`) becomes a comment naming the
    /// tag: a bound attribute's expression is emitted by the HTML scanner
    /// either way, so nothing the type engine could use is lost.
    ///
    /// `rest` is the remainder of the line after the tag name and
    /// `following` the lines after it, which the attribute list may run
    /// into.
    fn emit(
        &self,
        target: Option<ComponentTarget>,
        rest: &[char],
        following: &[&str],
    ) -> (String, Option<OpenComponentCall>) {
        let Some(target) = target else {
            return (
                format!(
                    " /* {slash}{prefix}{name} */ ",
                    slash = if self.closing { "/" } else { "" },
                    prefix = self.prefix,
                    name = self.name,
                ),
                None,
            );
        };
        let fqn = target.fqn.trim_matches('\\');
        let var = super::COMPONENT_VAR;
        let (parameters, mount) = match &target.binding {
            ComponentBinding::Construct(parameters) => (parameters, false),
            ComponentBinding::Mount(parameters) => (parameters, true),
            // A component whose attributes are arguments to nothing (an
            // anonymous one, whose attributes are its *view's* variables)
            // just declares the variable.
            ComponentBinding::Declare => {
                return (format!(" /** @var \\{fqn} ${var} */ ${var} = null; "), None);
            }
        };
        // The plain attributes have to be read ahead: their markup stays
        // masked where it is written, so there is nowhere to emit them but
        // the call itself. A tag whose `>` is nowhere to be found has no
        // call to emit at all.
        let Some(literals) = tag_attribute_arguments(rest, following) else {
            return (format!(" /** @var \\{fqn} ${var} */ ${var} = null; "), None);
        };

        let call = OpenComponentCall {
            fqn: fqn.to_string(),
            mount,
            pending: parameters.clone(),
            literals,
            arguments: Vec::new(),
        };
        (String::new(), Some(call))
    }

    /// Whether the tag name is one Blade's compiler claims for itself
    /// rather than looking up as a component: `<x-slot>` / `<x-slot:name>`
    /// open a slot on the surrounding component, and
    /// `<x-dynamic-component>` names its target through a `:component`
    /// attribute the surrounding scan already emits.  A project component
    /// that happened to share one of those names would never be reached
    /// by the tag anyway.
    fn is_reserved(&self) -> bool {
        self.prefix == "x-"
            && matches!(
                self.name
                    .split_once(':')
                    .map_or(self.name.as_str(), |(head, _)| head),
                "slot" | "dynamic-component"
            )
    }
}

/// The prefix of the variables a component tag's bound attributes are
/// bound to before the call that consumes them.
///
/// They are the preprocessor's own, so they are exempt from the
/// unused-variable diagnostic the way `$loop` and `$component` are.
pub const ARGUMENT_VAR_PREFIX: &str = "__blade_arg_";

/// The call a resolved component tag makes, held between the tag's name
/// and the `>` that closes it.
struct OpenComponentCall {
    /// Fully qualified class name, without a leading `\`.
    fqn: String,
    /// Whether the attributes are `mount()`'s arguments rather than the
    /// constructor's.
    mount: bool,
    /// The parameters no attribute has filled yet, in declaration order.
    pending: Vec<ComponentParameter>,
    /// The tag's plain attributes as `(camelCase name, PHP expression)`,
    /// read ahead when the tag opened, since their markup stays masked
    /// where it is written.
    literals: Vec<(String, String)>,
    /// The `name: value` arguments settled so far, in the order the
    /// attributes filling them were written.
    arguments: Vec<String>,
}

impl OpenComponentCall {
    /// Claim the parameter a bound attribute named `attr` fills, and
    /// return the variable its expression is bound to. `None` when the
    /// attribute names no parameter — Laravel routes that one to the
    /// component's attribute bag instead.
    ///
    /// A bound attribute claims ahead of a plain one of the same name
    /// (which is duplicate markup either way), so that this and the scan
    /// in [`super::component_tags`] agree on which attributes are
    /// arguments without either having to know the order the other saw
    /// them in.
    fn take(&mut self, attr: &str) -> Option<String> {
        let index = self.pending.iter().position(|param| param.name == attr)?;
        let param = self.pending.remove(index);
        let variable = format!("${ARGUMENT_VAR_PREFIX}{}", param.name);
        self.arguments.push(format!("{}: {variable}", param.name));
        Some(variable)
    }

    /// The whole call: the arguments the tag's attributes settled, then
    /// what Laravel itself would pass for a parameter no attribute
    /// filled.
    fn close(mut self) -> String {
        for (name, value) in std::mem::take(&mut self.literals) {
            if self.pending.iter().any(|param| param.name == name) {
                self.pending.retain(|param| param.name != name);
                self.arguments.push(format!("{name}: {value}"));
            }
        }
        for param in &self.pending {
            if let Some(fallback) = &param.fallback {
                self.arguments.push(format!("{}: {fallback}", param.name));
            }
        }
        let var = super::COMPONENT_VAR;
        let arguments = self.arguments.join(", ");
        if self.mount {
            format!(
                " ${var} = new \\{}(); ${var}->mount({arguments}); ",
                self.fqn
            )
        } else {
            format!(" ${var} = new \\{}({arguments}); ", self.fqn)
        }
    }
}

/// How far a tag's attribute list is followed onto later lines before it
/// is given up on. Well past any real tag, and it bounds the scan on a
/// template whose `<` never closes.
const MAX_TAG_LOOKAHEAD_LINES: usize = 64;

/// The plain attributes of the tag whose name ends at `rest`, as
/// `(camelCase name, PHP expression)`, or `None` when the tag's `>` is
/// nowhere to be found and there is therefore nowhere to emit its call.
///
/// Bound attributes (`:name="$expr"`, `:$name`) are left out: their
/// expression is emitted where it is written, so that hovering it still
/// lands on the template's own text.
fn tag_attribute_arguments(rest: &[char], following: &[&str]) -> Option<Vec<(String, String)>> {
    let mut chars: Vec<char> = rest.to_vec();
    for line in following.iter().take(MAX_TAG_LOOKAHEAD_LINES) {
        chars.push('\n');
        chars.extend(line.chars());
    }

    let mut attributes = Vec::new();
    let mut i = 0;
    // Whether the previous character ended a token, so the next name is at
    // attribute position rather than in the middle of a value.
    let mut at_attribute = true;
    while i < chars.len() {
        let rem = &chars[i..];
        let ch = chars[i];
        if ch == '>' {
            return Some(attributes);
        }
        if ch == '/' && rem.get(1) == Some(&'>') {
            return Some(attributes);
        }
        if ch.is_whitespace() {
            at_attribute = true;
            i += 1;
            continue;
        }
        if !at_attribute {
            i += 1;
            continue;
        }
        at_attribute = false;

        // A bound attribute is emitted where it is written; only its name
        // has to be stepped over here.
        let bound = ch == ':' && rem.get(1) != Some(&':');
        let name_start = i + usize::from(ch == ':');
        let mut end = name_start;
        while end < chars.len() && is_attr_name_char(chars[end]) {
            end += 1;
        }
        if end == name_start {
            i += 1;
            continue;
        }
        let name = super::component_tags::camel_case_attr_name(
            &chars[name_start..end].iter().collect::<String>(),
        );
        i = end;

        if chars.get(i) != Some(&'=') {
            // A bare attribute is `true`; a bare *bound* one is markup
            // Blade drops.
            if !bound {
                attributes.push((name, "true".to_string()));
            }
            continue;
        }
        i += 1;

        let quote = chars.get(i).copied().filter(|c| *c == '"' || *c == '\'');
        let value_start = i + usize::from(quote.is_some());
        let mut end = value_start;
        match quote {
            Some(delimiter) => {
                while end < chars.len() && chars[end] != delimiter {
                    end += 1;
                }
                if end == chars.len() {
                    // The value never closes: the tag is malformed and its
                    // `>` cannot be trusted either.
                    return None;
                }
            }
            None => {
                while end < chars.len() && !chars[end].is_whitespace() && chars[end] != '>' {
                    end += 1;
                }
            }
        }
        let value: String = chars[value_start..end].iter().collect();
        if !bound {
            // An attribute value carrying an echo is whatever the echo
            // renders concatenated with the text around it, which is a
            // string and nothing more precise. The echo's own expression
            // is emitted where it is written, as it is on any other tag.
            let value = if value.contains("{{") || value.contains("{!!") {
                "(string) ''".to_string()
            } else {
                php_string_literal(&value)
            };
            attributes.push((name, value));
        }
        i = end + usize::from(quote.is_some());
    }
    None
}

/// The characters an HTML attribute name is spelled with, which is a
/// wider set than a tag name's: `wire:model.live`, `x-on:keydown`, and
/// `@click` are all legal there.
fn is_attr_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '_' | '@')
}

/// `text` as a single-quoted PHP string literal.
fn php_string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        if ch == '\'' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

/// The component tag `rem` opens, or `None` when `rem` is not one.
fn component_tag_at(rem: &[char]) -> Option<ComponentTag> {
    if rem.first() != Some(&'<') {
        return None;
    }
    let closing = rem.get(1) == Some(&'/');
    let after_angle = 1 + usize::from(closing);
    for prefix in TAG_PREFIXES {
        if !starts_with_ascii(&rem[after_angle..], prefix) {
            continue;
        }
        let name_start = after_angle + prefix.len();
        let mut end = name_start;
        while end < rem.len() && is_tag_name_char(rem[end]) {
            end += 1;
        }
        if end == name_start {
            return None;
        }
        return Some(ComponentTag {
            len: end,
            prefix,
            name: rem[name_start..end].iter().collect(),
            closing,
        });
    }
    None
}

/// Whether `chars` opens with the ASCII `prefix`, without allocating: the
/// HTML scan asks this of every `<` in the template.
fn starts_with_ascii(chars: &[char], prefix: &str) -> bool {
    chars.len() >= prefix.len()
        && chars
            .iter()
            .zip(prefix.bytes())
            .all(|(ch, byte)| *ch == byte as char)
}

/// The characters a component tag name is spelled with. Dots separate
/// directories (`forms.input`), a double colon a package namespace
/// (`pkg::calendar`), and `<x-slot:title>` names a slot the same way.
fn is_tag_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '_')
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

/// Whether `needle` occurs anywhere in `haystack`.
fn contains_seq(haystack: &[char], needle: &[char]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Whether a bound attribute delimited by `quote` closes on a line after
/// the one it opens on. `rest` is the remainder of the opening line (after
/// the opening quote) and `following` the lines after it.
///
/// `false` covers both the single-line case and a malformed attribute whose
/// closing quote never appears, so the caller closes the expression at end
/// of line in either case. A malformed attribute can still pick up a quote
/// from further down the template, but that markup is already broken.
fn bound_attr_spans_lines(quote: char, rest: &[char], following: &[&str]) -> bool {
    let mut in_string = None;
    let mut is_escaped = false;
    if scan_to_bound_attr_end(quote, rest.iter().copied(), &mut in_string, &mut is_escaped) {
        return false;
    }
    following
        .iter()
        .any(|line| scan_to_bound_attr_end(quote, line.chars(), &mut in_string, &mut is_escaped))
}

/// Scan one line's worth of a bound-attribute expression, reporting whether
/// the closing `quote` was reached. `in_string` and `is_escaped` carry the
/// PHP string state into the next line and must mirror how the main scan
/// tracks it, or the two disagree about where the attribute ends.
fn scan_to_bound_attr_end(
    quote: char,
    chars: impl Iterator<Item = char>,
    in_string: &mut Option<char>,
    is_escaped: &mut bool,
) -> bool {
    for ch in chars {
        match *in_string {
            _ if ch == quote && in_string.is_none() => return true,
            Some(delim) => {
                if *is_escaped {
                    *is_escaped = false;
                } else if ch == '\\' {
                    *is_escaped = true;
                } else if ch == delim {
                    *in_string = None;
                }
            }
            None => {
                if ch == '\'' || ch == '"' {
                    *in_string = Some(ch);
                }
            }
        }
    }
    false
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

    /// A template that renders with a component instance bound wraps its
    /// body in a method of a subclass of that component, which is the only
    /// way `$this` can carry a type: PHP allows neither `$this = …` nor
    /// `global $this`.
    #[test]
    fn test_a_bound_this_wraps_the_body_in_a_subclass_method() {
        let (php, map) = preprocess_with_vars(
            "{{ $this->count }}",
            &[],
            TemplateKind::View,
            Some("App\\Livewire\\Counter"),
            None,
            &Default::default(),
        );
        assert!(
            php.contains(
                "abstract class __blade_scope_App_Livewire_Counter \
                 extends \\App\\Livewire\\Counter \
                 { public function __blade_template() { global $errors, $__env;"
            ),
            "the body must sit in a method of a subclass of the component: {}",
            php
        );
        assert!(
            php.trim_end().ends_with("} }"),
            "the method and the class holding it must both close: {}",
            php
        );
        // The wrapper still occupies exactly one prologue line, so Blade
        // positions map the same as they do without a bound `$this`.
        assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES);
    }

    #[test]
    fn test_component_prologue_declares_attributes_and_slot() {
        let (php, map) = preprocess_with_vars(
            "<img {{ $attributes->merge(['class' => 'x']) }} />{{ $slot }}",
            &[],
            TemplateKind::Component,
            None,
            None,
            &Default::default(),
        );
        assert!(
            php.contains("/** @var \\Illuminate\\View\\ComponentAttributeBag $attributes */")
                && php.contains("/** @var \\Illuminate\\View\\ComponentSlot $slot */"),
            "component variables must be declared with their framework types: {}",
            php
        );
        assert!(
            php.contains("/** @var string $componentName */"),
            "a component also knows its own name: {}",
            php
        );
        assert!(
            php.contains(
                "function __blade_template() { global $errors, $__env, $attributes, $slot, $componentName;"
            ),
            "component variables must be pulled into the wrapper scope: {}",
            php
        );
        // Three declarations of two lines each on top of the base prologue.
        assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 6);
    }

    #[test]
    fn test_plain_view_prologue_has_no_component_variables() {
        let (php, _) = preprocess("{{ $slot }}");
        assert!(
            !php.contains("$attributes = new") && !php.contains("$slot = new"),
            "a plain view must not receive component variables: {}",
            php
        );
    }

    /// A caller cannot pass `$attributes`, so a call-site inference that
    /// produced one must not overwrite the framework's own declaration.
    #[test]
    fn test_component_variables_are_not_overwritten_by_inferred_vars() {
        let (php, map) = preprocess_with_vars(
            "{{ $attributes }}",
            &[("attributes".to_string(), "string".to_string())],
            TemplateKind::Component,
            None,
            None,
            &Default::default(),
        );
        assert!(
            !php.contains("$attributes = null;"),
            "the inferred declaration must be dropped: {}",
            php
        );
        assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 6);
    }

    #[test]
    fn test_preprocess_with_vars_injects_declarations() {
        let content = "{{ $user->name }}";
        let (php, map) = preprocess_with_vars(
            content,
            &[
                ("results".to_string(), "array<int, string>".to_string()),
                ("user".to_string(), "\\App\\Models\\User".to_string()),
            ],
            TemplateKind::View,
            None,
            None,
            &Default::default(),
        );
        assert!(
            php.contains("/** @var array<int, string> $results */"),
            "injected @var declaration missing: {}",
            php
        );
        assert!(
            php.contains("/** @var \\App\\Models\\User $user */"),
            "injected @var declaration missing: {}",
            php
        );
        assert!(
            php.contains("function __blade_template() { global $errors, $__env, $results, $user;"),
            "injected variables must be pulled into the wrapper scope: {}",
            php
        );
        // Each injected variable adds a @var line and an assignment line.
        assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 4);

        // Round trip: blade (0,0) → php and back lands on the same line.
        let php_pos = map.blade_to_php(tower_lsp::lsp_types::Position {
            line: 0,
            character: 3,
        });
        assert_eq!(php_pos.line, map.prologue_lines);
        let back = map.php_to_blade(php_pos);
        assert_eq!(back.line, 0);
    }

    #[test]
    fn test_preprocess_without_vars_keeps_default_prologue() {
        let (_, map) = preprocess("{{ $x }}");
        assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES);
    }

    /// A literal-string type keeps its source form, and PHP allows a real
    /// line break inside a quoted string — so an inferred type can arrive
    /// with a newline in it. It must not add a prologue line (that would
    /// shift every position in the template) nor leave the `@var` docblock
    /// straddling two lines.
    #[test]
    fn test_preprocess_with_vars_multiline_type_does_not_shift_positions() {
        let (php, map) = preprocess_with_vars(
            "{{ $body }}",
            &[("body".to_string(), "'line1\nline2'".to_string())],
            TemplateKind::View,
            None,
            None,
            &Default::default(),
        );
        assert_eq!(map.prologue_lines, super::super::PROLOGUE_LINES + 2);
        assert!(
            php.contains("/** @var mixed $body */"),
            "a multi-line type must degrade to mixed: {}",
            php
        );
        // The template body still starts exactly at the prologue height.
        let php_lines: Vec<&str> = php.lines().collect();
        assert!(
            php_lines[map.prologue_lines as usize].contains("$body"),
            "template line 0 must sit at prologue_lines: {}",
            php
        );
    }

    /// A `*/` inside an inferred type would close the docblock early and
    /// spill the remainder into code.
    #[test]
    fn test_preprocess_with_vars_type_cannot_close_the_docblock() {
        let (php, _) = preprocess_with_vars(
            "{{ $x }}",
            &[("x".to_string(), "'*/ evil()'".to_string())],
            TemplateKind::View,
            None,
            None,
            &Default::default(),
        );
        assert!(
            php.contains("/** @var mixed $x */") && !php.contains("evil()"),
            "a type containing */ must degrade to mixed: {}",
            php
        );
    }

    /// A component tag's attribute names are HTML, so a caller writing
    /// `wire:model.live="…"` or `@click="…"` offers a name PHP cannot bind.
    /// Blade's `extract()` skips those keys, and so must the prologue:
    /// emitting `$wire:model.live = null;` would be a syntax error that
    /// takes the whole template down with it.
    #[test]
    fn test_preprocess_with_vars_skips_names_php_cannot_bind() {
        let (php, _) = preprocess_with_vars(
            "{{ $ok }}",
            &[
                ("wire:model.live".to_string(), "string".to_string()),
                ("@click".to_string(), "string".to_string()),
                ("ok".to_string(), "string".to_string()),
            ],
            TemplateKind::Component,
            None,
            None,
            &Default::default(),
        );
        assert!(
            !php.contains("wire:model.live") && !php.contains("@click"),
            "an attribute name that is not a PHP variable must not be declared: {}",
            php
        );
        assert!(
            php.contains("$ok = null;") && php.contains(", $ok;"),
            "a valid name alongside it must still be declared: {}",
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
            php.contains("blade_stack_directive ('scripts');"),
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

    /// The directives a project registered, as the provider scan would have
    /// recorded them.
    fn registered(names: &[(&str, bool)]) -> CustomDirectives {
        let registrations: Vec<super::super::directives::CustomDirective> = names
            .iter()
            .map(
                |(name, conditional)| super::super::directives::CustomDirective {
                    name: name.to_string(),
                    conditional: *conditional,
                },
            )
            .collect();
        CustomDirectives::from_registrations(&registrations)
    }

    /// The wrapped template body of a preprocessed template, without the
    /// prologue — whose marker declarations would otherwise answer a search
    /// for a marker the body never calls.
    fn preprocess_with_directives(content: &str, directives: &CustomDirectives) -> String {
        let (php, _) =
            preprocess_with_vars(content, &[], TemplateKind::View, None, None, directives);
        let body_start = php
            .find("global $errors")
            .expect("wrapper function prologue");
        php[body_start..].to_string()
    }

    /// A `Blade::directive()` registration is a statement whose argument the
    /// template still gets type-checked on, rather than the comment an
    /// unregistered `@name` degrades to.
    #[test]
    fn a_registered_directive_keeps_its_argument_as_real_php() {
        let php = preprocess_with_directives(
            "<p>@datetime($post->createdAt)</p>",
            &registered(&[("datetime", false)]),
        );
        assert!(
            php.contains("blade_custom_directive ($post->createdAt);"),
            "unexpected @datetime translation: {php}"
        );
    }

    /// Blade hands a handler an empty expression when the template writes no
    /// argument list, so a bare name must complete on the spot instead of
    /// scanning ahead for a closing paren that was never opened.
    #[test]
    fn a_registered_directive_without_arguments_stands_alone() {
        let php = preprocess_with_directives(
            "<p>@datetime</p>{{ $after }}",
            &registered(&[("datetime", false)]),
        );
        assert!(
            php.contains("blade_custom_directive();"),
            "unexpected bare @datetime translation: {php}"
        );
        assert!(
            php.contains("echo e( $after"),
            "the rest of the template must still be scanned as Blade: {php}"
        );
    }

    /// `Blade::if('admin')` gives the template four directives, and the
    /// three that are not the `@end` open a real condition so the `@endadmin`
    /// closing them balances.
    #[test]
    fn a_registered_condition_opens_a_real_if() {
        let php = preprocess_with_directives(
            "@admin('editor')\n<p>yes</p>\n@elseadmin('viewer')\n<p>maybe</p>\n@endadmin\n<p>after</p>",
            &registered(&[("admin", true)]),
        );
        assert!(
            php.contains("if (blade_custom_directive ('editor')):"),
            "@admin should open a balanced if: {php}"
        );
        assert!(
            php.contains("elseif (blade_custom_directive ('viewer')):"),
            "@elseadmin should open a balanced elseif: {php}"
        );
        assert!(
            php.contains("endif;"),
            "@endadmin should close what @admin opened: {php}"
        );
    }

    /// The argument list is optional for every member of the family, and
    /// `@unlessadmin` is closed by the same `@endadmin` as `@admin`.
    #[test]
    fn a_registered_condition_without_arguments_is_still_balanced() {
        let php = preprocess_with_directives(
            "@unlessadmin\n<p>no</p>\n@endadmin\n",
            &registered(&[("admin", true)]),
        );
        assert!(
            php.contains("if (blade_custom_directive()):") && php.contains("endif;"),
            "a bare @unlessadmin must open and close a real if: {php}"
        );
    }

    /// Nothing was registered, so the directive is still what it always was:
    /// inert markup, with its argument not read as PHP at all.
    #[test]
    fn an_unregistered_directive_stays_masked() {
        let php = preprocess_with_directives("<p>@datetime($x)</p>", &CustomDirectives::default());
        assert!(
            !php.contains("blade_custom_directive") && !php.contains("$x"),
            "an unregistered directive must stay masked: {php}"
        );
    }

    /// Blade's compiler consults its custom table before its own directives,
    /// but a registration shadowing a core name would break the block
    /// structure of every template that writes it, so the core table wins.
    #[test]
    fn a_registration_does_not_shadow_a_core_directive() {
        let php = preprocess_with_directives(
            "@if ($ok)\n<p>hi</p>\n@endif\n",
            &registered(&[("if", false), ("endif", false)]),
        );
        assert!(
            php.contains("($ok):") && !php.contains("blade_custom_directive"),
            "@if must still compile as Blade's own directive: {php}"
        );
    }

    /// `@can`/`@cannot`/`@canany` (and their `@elsecan*` counterparts) open a
    /// real `if`/`elseif` so the always-literal `@endif` that closes them
    /// stays balanced, while their arguments are still type-checked.
    #[test]
    fn test_preprocess_can_directive_opens_a_real_if() {
        let content = "@can('update', $post)\n<p>can</p>\n@elsecan('view', $post)\n<p>view</p>\n@endcan\n<p>after</p>";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("if (blade_can_directive ('update', $post)):"),
            "@can should open a balanced if with its arguments type-checked: {}",
            php
        );
        assert!(
            php.contains("elseif (blade_can_directive ('view', $post)):"),
            "@elsecan should open a balanced elseif with its arguments type-checked: {}",
            php
        );
        assert!(
            php.contains("endif;"),
            "@endcan should close the if opened by @can: {}",
            php
        );
        assert!(
            !php.contains("after"),
            "content after @endcan should be masked as HTML, not left as raw PHP: {}",
            php
        );
    }

    /// `@hasStack`/`@hasSection`/`@sectionMissing` are always closed by a
    /// literal `@endif`, not a dedicated end-directive, so they must open a
    /// real `if` too (previously they degraded to a bare comment, leaving
    /// `@endif` dangling with no matching `if` and breaking the rest of the
    /// virtual PHP file).
    #[test]
    fn test_preprocess_has_stack_and_has_section_open_a_real_if() {
        let (php, _) = preprocess("@hasStack('scripts')\nx\n@endif\n<p>after</p>");
        assert!(
            php.contains("if (blade_stack_directive ('scripts')):"),
            "@hasStack should open a balanced if with its argument type-checked: {}",
            php
        );
        assert!(
            !php.contains("after"),
            "content after @endif should be masked as HTML, not left as raw PHP: {}",
            php
        );

        let (php, _) = preprocess("@hasSection('content')\nx\n@endif\n<p>after</p>");
        assert!(
            php.contains("if (blade_section_directive ('content')):"),
            "@hasSection should open a balanced if with its argument type-checked: {}",
            php
        );

        let (php, _) = preprocess("@sectionMissing('content')\nx\n@endif\n<p>after</p>");
        assert!(
            php.contains("if (blade_section_directive ('content')):"),
            "@sectionMissing should open a balanced if with its argument type-checked: {}",
            php
        );
    }

    /// `@pushIf`/`@pushOnce`/`@prependOnce`/`@hasStack` used to fall through
    /// to `translate_directive`'s default `/* @directive */` comment, which
    /// left their arguments completely untyped.
    #[test]
    fn test_preprocess_push_if_and_push_once_consume_arguments() {
        let (php, _) = preprocess("@pushIf($condition, 'scripts')\nx\n@endPushIf\n<p>after</p>");
        assert!(
            php.contains("blade_push_if_directive ($condition, 'scripts');"),
            "@pushIf should type-check its arguments: {}",
            php
        );

        let (php, _) = preprocess("@pushOnce('scripts')\nx\n@endPushOnce\n<p>after</p>");
        assert!(
            php.contains("blade_stack_directive ('scripts');"),
            "@pushOnce should type-check its argument: {}",
            php
        );

        let (php, _) = preprocess("@prependOnce('scripts')\nx\n@endPrependOnce\n<p>after</p>");
        assert!(
            php.contains("blade_stack_directive ('scripts');"),
            "@prependOnce should type-check its argument: {}",
            php
        );
    }

    /// `@lang` is optional-argument: bare it opens a translation-buffering
    /// block paired with `@endlang` (nothing to type-check), and with an
    /// argument it is a one-shot call whose expression should be checked.
    #[test]
    fn test_preprocess_lang_directive_optional_argument() {
        let (php, _) = preprocess("@lang\n<p>x</p>\n@endlang\n<p>after</p>");
        assert!(
            !php.contains("after"),
            "bare @lang/@endlang should not swallow the rest of the template into raw PHP: {}",
            php
        );

        let (php, _) = preprocess("@lang($key)\n<p>after</p>");
        assert!(
            php.contains("blade_directive ($key);"),
            "@lang(...) should type-check its argument: {}",
            php
        );
    }

    /// `@vite`/`@fonts` take an optional argument list; a bare `@vite` must
    /// not send the scanner hunting for a closing paren that was never
    /// opened, which would swallow the rest of the template.
    #[test]
    fn test_preprocess_vite_and_fonts_optional_argument() {
        let (php, _) = preprocess("@vite\n<p>after</p>");
        assert!(
            !php.contains("after"),
            "bare @vite should not swallow the rest of the template into raw PHP: {}",
            php
        );

        let (php, _) = preprocess("@vite(['resources/js/app.js'])\n<p>after</p>");
        assert!(
            php.contains("blade_directive (['resources/js/app.js']);"),
            "@vite(...) should type-check its argument: {}",
            php
        );

        let (php, _) = preprocess("@fonts\n<p>after</p>");
        assert!(
            !php.contains("after"),
            "bare @fonts should not swallow the rest of the template into raw PHP: {}",
            php
        );
    }

    /// `@unset($var)` must compile to a real `unset(...)` statement, not a
    /// `blade_directive(...)` call — `unset` is a language construct and
    /// cannot be used as a function-call argument.
    #[test]
    fn test_preprocess_unset_directive() {
        let (php, _) = preprocess("@unset($value)\n<p>after</p>");
        assert!(
            php.contains("unset ($value);"),
            "@unset should compile to a real unset() statement: {}",
            php
        );
    }

    /// `@choice`/`@js`/`@dd`, previously unrecognised entirely (masked as
    /// inert HTML), must type-check their arguments like other expression
    /// directives.
    #[test]
    fn test_preprocess_choice_js_dd_directives_consume_arguments() {
        let (php, _) = preprocess("@choice('apples', $count)\n<p>after</p>");
        assert!(
            php.contains("blade_directive ('apples', $count);"),
            "@choice should type-check its arguments: {}",
            php
        );

        let (php, _) = preprocess("@js($data)\n<p>after</p>");
        assert!(
            php.contains("blade_directive ($data);"),
            "@js should type-check its argument: {}",
            php
        );

        let (php, _) = preprocess("@dd($value)\n<p>after</p>");
        assert!(
            php.contains("blade_directive ($value);"),
            "@dd should type-check its argument: {}",
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
            php.contains("blade_bound_attr_directive($image);"),
            "bound attribute expression should be emitted as PHP: {}",
            php
        );
        // A tag no component index resolves becomes a comment naming it,
        // never executable PHP.
        assert!(
            php.contains("/* x-img.size */"),
            "an unresolved tag should degrade to a comment: {}",
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
            php.contains("blade_bound_attr_directive($item->id);"),
            "method-call expression in a bound attribute should be emitted: {}",
            php
        );
        // The `:` inside the `livewire:edit-channel` tag name is part of
        // the name, not an attribute, so it must not open a directive call.
        assert!(
            !php.contains("blade_bound_attr_directive(edit-channel"),
            "namespace colon in the tag name must not be treated as a binding: {}",
            php
        );
    }

    /// A resolver over a fixed table, standing in for the project's
    /// discovery index.
    struct TestComponents(Vec<(String, ComponentTarget)>);

    impl ComponentResolver for TestComponents {
        fn x_component(&self, tag: &str) -> Option<ComponentTarget> {
            self.lookup(&format!("x-{tag}"))
        }

        fn livewire_component(&self, name: &str) -> Option<ComponentTarget> {
            self.lookup(&format!("livewire:{name}"))
        }
    }

    impl TestComponents {
        fn lookup(&self, tag: &str) -> Option<ComponentTarget> {
            self.0
                .iter()
                .find(|(known, _)| known == tag)
                .map(|(_, target)| target.clone())
        }
    }

    /// A parameter list from `name` / `name?` / `name=fallback` spellings:
    /// required with nothing to stand in for it, optional, and required
    /// with what Laravel's container would pass.
    fn params(spec: &[&str]) -> Vec<ComponentParameter> {
        spec.iter()
            .map(|entry| match entry.split_once('=') {
                Some((name, fallback)) => ComponentParameter {
                    name: name.to_string(),
                    fallback: Some(fallback.to_string()),
                },
                None => ComponentParameter {
                    name: entry.trim_end_matches('?').to_string(),
                    fallback: None,
                },
            })
            .collect()
    }

    fn target(fqn: &str, binding: ComponentBinding) -> ComponentTarget {
        ComponentTarget {
            fqn: fqn.to_string(),
            binding,
        }
    }

    fn preprocess_with_components(
        content: &str,
        components: Vec<(String, ComponentTarget)>,
    ) -> String {
        let resolver = TestComponents(components);
        preprocess_with_vars(
            content,
            &[],
            TemplateKind::View,
            None,
            Some(&resolver as &dyn ComponentResolver),
            &Default::default(),
        )
        .0
    }

    /// `Alert::__construct(string $type, ?Post $post = null)`.
    fn alert() -> Vec<(String, ComponentTarget)> {
        vec![(
            "x-alert".to_string(),
            target(
                "App\\View\\Components\\Alert",
                ComponentBinding::Construct(params(&["type", "post?"])),
            ),
        )]
    }

    /// A tag the component index knows binds `$component` to the class
    /// behind it, and its attributes become the arguments Laravel builds
    /// that class with, so a component handed the wrong thing is reported
    /// the way any other call is.
    #[test]
    fn test_preprocess_component_tag_builds_the_component() {
        let php =
            preprocess_with_components(r#"<x-alert type="danger">{{ $slot }}</x-alert>"#, alert());
        assert!(
            php.contains("$component = new \\App\\View\\Components\\Alert("),
            "the tag should build its component: {php}"
        );
        assert!(
            php.contains("type: 'danger'"),
            "a plain attribute becomes a named argument: {php}"
        );
        assert!(
            php.contains("/* /x-alert */"),
            "the closing tag should become a comment: {php}"
        );
        assert!(
            !php.contains(r#"type="danger""#),
            "attribute markup should stay masked: {php}"
        );
    }

    /// A bound attribute's expression is the argument, emitted where the
    /// template wrote it so that hovering it lands on the template's own
    /// text. An attribute naming no parameter is markup Laravel routes to
    /// the component's attribute bag, so it is not an argument at all --
    /// but its expression is still emitted, or a variable used only there
    /// would read as unused.
    #[test]
    fn test_preprocess_component_tag_partitions_its_attributes() {
        let php = preprocess_with_components(
            r#"<x-alert :type="$kind" class="m-2" :data-id="$id" />"#,
            alert(),
        );
        assert!(
            php.contains("$__blade_arg_type = $kind;") && php.contains("type: $__blade_arg_type"),
            "a bound attribute naming a parameter is an argument: {php}"
        );
        assert!(
            !php.contains("class:") && !php.contains("dataId:"),
            "an attribute the attribute bag takes is not an argument: {php}"
        );
        assert!(
            php.contains("blade_bound_attr_directive($id);"),
            "a bound attribute that is not an argument still contributes \
             its expression: {php}"
        );
    }

    /// Laravel builds a component the tag left incomplete through the
    /// container, so a parameter no attribute filled is passed what the
    /// container would pass rather than being reported missing. One
    /// nothing can stand in for is left out, which is the case Laravel
    /// itself fails on.
    #[test]
    fn test_preprocess_component_tag_fills_what_the_container_would() {
        let components = vec![(
            "x-card".to_string(),
            target(
                "App\\View\\Components\\Card",
                ComponentBinding::Construct(params(&[
                    "title",
                    "footer?",
                    "service=resolve(\\App\\Service::class)",
                    "count=null",
                ])),
            ),
        )];
        let php = preprocess_with_components("<x-card />", components);
        assert!(
            php.contains("service: resolve(\\App\\Service::class)") && php.contains("count: null"),
            "the container's own arguments should be passed: {php}"
        );
        assert!(
            !php.contains("title:") && !php.contains("footer:"),
            "a parameter with a default and one nothing can fill are both \
             left out: {php}"
        );
    }

    /// A dotted tag names a component in a sub-directory, and an index
    /// component answers to its directory alone; both are ordinary index
    /// lookups, so the preprocessor only has to pass the name through
    /// unchanged.
    #[test]
    fn test_preprocess_component_tag_passes_the_written_name_through() {
        let nested = [
            ("x-forms.input", "App\\View\\Components\\Forms\\Input"),
            ("x-card", "App\\View\\Components\\Card\\Card"),
            ("x-pkg::calendar", "Vendor\\Pkg\\Calendar"),
        ];
        let components: Vec<(String, ComponentTarget)> = nested
            .iter()
            .map(|(tag, fqn)| {
                (
                    (*tag).to_string(),
                    target(fqn, ComponentBinding::Construct(Vec::new())),
                )
            })
            .collect();
        for (tag, fqn) in nested {
            let php = preprocess_with_components(&format!("<{tag} />"), components.clone());
            assert!(
                php.contains(&format!("$component = new \\{fqn}(")),
                "<{tag}> should resolve to {fqn}: {php}"
            );
        }
    }

    /// Livewire builds its component through the container and hands the
    /// tag's attributes to `mount()`.
    #[test]
    fn test_preprocess_livewire_tag_mounts_the_component() {
        let components = vec![(
            "livewire:counter".to_string(),
            target(
                "App\\Livewire\\Counter",
                ComponentBinding::Mount(params(&["count"])),
            ),
        )];
        let php = preprocess_with_components(r#"<livewire:counter :count="$n" />"#, components);
        assert!(
            php.contains("$component = new \\App\\Livewire\\Counter();"),
            "the tag should build its Livewire component: {php}"
        );
        assert!(
            php.contains("$component->mount(count: $__blade_arg_count);"),
            "the attributes are `mount()`'s arguments: {php}"
        );
    }

    /// A component whose attributes are arguments to nothing (an
    /// anonymous one, whose attributes are its *view's* variables) still
    /// declares `$component` so the tag body can reach it.
    #[test]
    fn test_preprocess_anonymous_component_declares_without_a_call() {
        let components = vec![(
            "x-banner".to_string(),
            target(super::super::ANONYMOUS_COMPONENT, ComponentBinding::Declare),
        )];
        let php = preprocess_with_components(r#"<x-banner :title="$t" />"#, components);
        assert!(
            php.contains(
                "/** @var \\Illuminate\\View\\AnonymousComponent $component */ $component = null;"
            ),
            "an anonymous component is declared, not built: {php}"
        );
        assert!(
            php.contains("blade_bound_attr_directive($t);"),
            "its attributes still contribute their expressions: {php}"
        );
    }

    /// Markup between a tag's name and its `>` becomes statements — a
    /// `{{ }}` echo in an attribute value, a directive, a bound attribute
    /// the attribute bag takes — which is why the call is emitted where
    /// the tag closes rather than spanning it. The statements stand, and
    /// the call still happens.
    #[test]
    fn test_preprocess_component_tag_survives_markup_between_its_attributes() {
        let php = preprocess_with_components(r#"<x-alert type="a {{ $kind }}" />"#, alert());
        assert!(
            php.contains("echo e( $kind );"),
            "the echo in the attribute value still runs: {php}"
        );
        assert!(
            php.contains("new \\App\\View\\Components\\Alert(type: (string) '')"),
            "an interpolated value is a string and nothing more precise: {php}"
        );

        let php = preprocess_with_components("<x-alert @if($a) type=\"x\" @endif />", alert());
        assert!(
            php.contains("if ($a):") && php.contains("new \\App\\View\\Components\\Alert("),
            "a directive between attributes is still a directive: {php}"
        );
    }

    /// A bound attribute is only at attribute position inside a tag, and
    /// the component tag's `<` never reaches the HTML scanner — so the
    /// tag state has to be carried across the replacement, including on a
    /// tag that spans lines.
    #[test]
    fn test_preprocess_component_tag_keeps_bound_attributes_in_scope() {
        let php =
            preprocess_with_components("<x-alert\n  :type=\"$kind\"\n/>\n:notAnAttr", alert());
        assert!(
            php.contains("$__blade_arg_type = $kind;") && php.contains("type: $__blade_arg_type"),
            "a bound attribute on a multi-line component tag: {php}"
        );
        assert!(
            !php.contains("blade_bound_attr_directive(notAnAttr"),
            "a colon after the tag closed is not an attribute: {php}"
        );
    }

    /// A tag name written inside an attribute value is markup, not a tag.
    #[test]
    fn test_preprocess_component_tag_inside_an_attribute_value_is_not_a_tag() {
        let php = preprocess_with_components(r#"<div data-tpl="<x-alert />"></div>"#, alert());
        assert!(
            !php.contains("$component ="),
            "a tag written inside an attribute value is not a tag: {php}"
        );
    }

    /// A tag no index resolves — an unknown component, `<x-slot>`, or
    /// `<x-dynamic-component>` — degrades to a comment, and a bound
    /// attribute on it still contributes its expression.
    #[test]
    fn test_preprocess_unresolved_component_tags_degrade_to_comments() {
        let php = preprocess_with_components(
            r#"<x-dynamic-component :component="$name" :attr="$v" /><x-slot:title>t</x-slot><x-unknown />"#,
            alert(),
        );
        for comment in [
            "/* x-dynamic-component */",
            "/* x-slot:title */",
            "/* x-unknown */",
        ] {
            assert!(php.contains(comment), "expected {comment} in: {php}");
        }
        assert!(
            !php.contains("$component ="),
            "no unresolved tag may bind a component: {php}"
        );
        assert!(
            php.contains("blade_bound_attr_directive($name);")
                && php.contains("blade_bound_attr_directive($v);"),
            "a dynamic component's expressions are still parsed: {php}"
        );
    }

    /// The `:$var` shorthand expands to a bound `var` attribute whose
    /// expression is `$var`.
    #[test]
    fn test_preprocess_bound_attribute_shorthand() {
        let content = r#"<x-alert :$message />"#;
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_bound_attr_directive($message);"),
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
            php.contains("blade_bound_attr_directive($active ? 'on' : 'off');"),
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
            php.contains("blade_bound_attr_directive($v);"),
            "the real binding should still be emitted: {}",
            php
        );
        // The prologue declares `function blade_bound_attr_directive(...)`
        // once, so a single binding yields two occurrences of
        // `blade_bound_attr_directive(`.
        assert_eq!(
            php.matches("blade_bound_attr_directive(").count(),
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
        // Only the prologue's `function blade_bound_attr_directive(...)`
        // declaration should remain; no binding call is emitted for a colon
        // in text.
        assert_eq!(
            php.matches("blade_bound_attr_directive(").count(),
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
            php.contains("blade_bound_attr_directive($image);"),
            "binding on a continuation line should be recognized: {}",
            php
        );
    }

    /// A bound attribute whose expression is wrapped over several lines
    /// (what a formatter does to a long array) must be emitted whole, not
    /// truncated at the first line break.
    #[test]
    fn test_preprocess_bound_attribute_multiline_expression() {
        let content = "<x-file.upload name=\"image\"\n    :rules=\"[\n        'Dimensions must match: 2420 x 1614',\n        'Max file size: 2 mb',\n    ]\" />\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_bound_attr_directive([") && php.contains("]);"),
            "the wrapped array must be emitted whole: {}",
            php
        );
        assert!(
            php.contains("'Dimensions must match: 2420 x 1614',"),
            "continuation lines must survive: {}",
            php
        );
        assert!(
            !php.contains("[);"),
            "the expression must not be closed off at the line break: {}",
            php
        );
        assert!(
            !php.contains("name=\"image\""),
            "the surrounding tag markup must stay masked: {}",
            php
        );
    }

    /// A multi-line bound attribute holding a call must keep every argument,
    /// otherwise the truncated call reports a bogus argument-count mismatch.
    #[test]
    fn test_preprocess_bound_attribute_multiline_call() {
        let content = "<x-alert\n    :message=\"__('a.b',\n        ['count' => 2])\"\n/>\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("__('a.b',") && php.contains("['count' => 2]));"),
            "both call arguments must survive the wrap: {}",
            php
        );
    }

    /// A bound attribute whose closing quote never appears is malformed;
    /// the call is closed at end of line so only that attribute is lost.
    #[test]
    fn test_preprocess_bound_attribute_unterminated() {
        let content = "<x-alert :message=\"$msg\n<p>{{ $after }}</p>\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_bound_attr_directive($msg);"),
            "an unterminated attribute must be closed at end of line: {}",
            php
        );
        assert!(
            php.contains("echo e( $after );"),
            "the rest of the template must still be processed: {}",
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

    /// An `@` before `{{ ... }}` leaves the expression untouched for a
    /// frontend template engine, even when the expression is not valid PHP.
    #[test]
    fn test_preprocess_at_escaped_echo_is_masked() {
        let content = "@{{.Image}} and {{ $serverValue }}";
        let (php, _) = preprocess(content);
        assert!(
            !php.contains(".Image"),
            "the frontend expression must stay out of virtual PHP: {php}"
        );
        assert!(
            php.contains("echo e( $serverValue );"),
            "a real Blade echo after it must still compile: {php}"
        );
    }

    /// Laravel's escaped-echo pattern spans lines and does not interpret
    /// quotes inside the frontend expression as PHP string delimiters.
    #[test]
    fn test_preprocess_at_escaped_echo_spans_lines() {
        let content = "@{{\n    frontend['unterminated]\n}}\n{{ $after }}";
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("frontend"),
            "the whole multiline frontend expression must be masked: {php}"
        );
        assert!(
            php.contains("echo e( $after );"),
            "processing must return to Blade after the escaped echo: {php}"
        );
    }

    /// Escaping a raw echo leaves its contents as frontend template text and
    /// resumes Blade processing after the raw `!!}` terminator.
    #[test]
    fn test_preprocess_at_escaped_raw_echo_is_masked() {
        let content = "@{!! $frontendName !!} and {!! $serverValue !!}";
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("$frontendName"),
            "the escaped raw expression must stay out of virtual PHP: {php}"
        );
        assert!(
            php.contains("echo  $serverValue ;"),
            "a real raw Blade echo after it must still compile: {php}"
        );
    }

    /// A raw `{!! … !!}` echo compiles to a naked `echo` with no `e()`
    /// wrapper, and it starts at `{!!`, not `{{!!`: `{!! $v !!}` after a
    /// `<?php $v = …; ?>` block must count as a use of `$v`.
    #[test]
    fn test_preprocess_raw_echo_single_brace() {
        let content = "<?php\n$acmeProfile = \"xxx\";\n?>\n\n{!! $acmeProfile !!}\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo  $acmeProfile ;"),
            "raw echo should emit a naked echo of the expression: {}",
            php
        );
    }

    /// Blade matches echo tags longest-opening-first, so `{{!! $v !!}}` is
    /// a literal `{`, a raw echo of `$v`, and a literal `}` — not an
    /// escaped echo of `!! $v !!`.
    #[test]
    fn test_preprocess_raw_echo_wrapped_in_extra_braces() {
        let content = "{{!! $html !!}}";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo  $html ;"),
            "the raw echo inside the extra braces should still compile: {}",
            php
        );
        assert!(
            !php.contains("echo e("),
            "the outer braces are literal text, not an escaped echo: {}",
            php
        );
    }

    #[test]
    fn test_preprocess_raw_and_escaped_echo_close_independently() {
        let content = "{!! $html !!} and {{ $safe }}";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo  $html ;"),
            "raw echo emits without e(): {}",
            php
        );
        assert!(
            php.contains("echo e( $safe );"),
            "escaped echo still wraps in e(): {}",
            php
        );
    }

    /// An echo opener that nothing in the rest of the file closes must not
    /// swallow every later line as PHP: it is closed at end of line, so at
    /// most one line degrades and the rest of the template still parses.
    #[test]
    fn test_preprocess_unterminated_echo_opener_is_closed_at_end_of_line() {
        let content = "<script>if (a) {!!b}</script>\n{{ $after }}\n<p>plain markup</p>\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo e( $after );"),
            "the echo on the next line must still compile: {}",
            php
        );
        assert!(
            !php.contains("plain markup"),
            "later markup must be masked as HTML, not emitted as PHP: {}",
            php
        );
        // The unclosed echo must be closed as a statement rather than
        // left to swallow the wrapper function's closing brace.
        assert!(
            php.contains("echo b}</script>; "),
            "the opener's own line degrades and is closed at its end: {}",
            php
        );
    }

    /// A half-typed echo is not an unpaired opener when a terminator exists
    /// further down (e.g. the user is typing inside an echo whose `!!}` is
    /// already there, or another echo's terminator follows): the expression
    /// must stay open so completion keeps working mid-edit.
    #[test]
    fn test_preprocess_echo_spanning_lines_stays_open() {
        let content = "{{ $user\n    ->name }}\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo e( $user"),
            "the multi-line echo must open: {}",
            php
        );
        assert!(
            php.contains("->name );"),
            "the multi-line echo must close at its own terminator: {}",
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
        assert!(
            !php.contains("$name"),
            "verbatim content should be skipped: {}",
            php
        );
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
        assert!(
            php.contains("function blade_each_directive"),
            "prologue should declare blade_each_directive: {}",
            php
        );
    }

    /// `@each` gets a marker of its own: the arguments after its view name
    /// are a collection and an item name, not a data array.
    #[test]
    fn test_preprocess_each_uses_its_own_marker() {
        let (php, _) = preprocess("@each('partials.row', $rows, 'row')\n");
        assert!(
            php.contains("blade_each_directive ('partials.row', $rows, 'row');"),
            "@each should compile to a blade_each_directive call: {}",
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
        // The import is hoisted into the prologue: top-level (not inside
        // the wrapper function) and ahead of every name it imports, since
        // name resolution runs in source order.
        let wrapper = php.find("function __blade_template()").unwrap();
        let import = php.find("use App\\Models\\Post;").unwrap();
        assert!(
            import < wrapper,
            "the use import must be hoisted into the prologue: {}",
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

    /// An apostrophe inside a `{{-- ... --}}` comment must not be mistaken
    /// for the start of a PHP string literal — that previously made the
    /// scanner hunt for a matching closing quote instead of the comment's
    /// `--}}` terminator, desyncing the rest of the file.
    #[test]
    fn test_preprocess_comment_with_apostrophe_does_not_desync() {
        let content = "{{-- user's note --}}\n<p>{{ $after }}</p>\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("/*  user's note"),
            "comment should translate to a block comment: {}",
            php
        );
        assert!(
            php.contains("echo e( $after )"),
            "content after the comment should still translate normally: {}",
            php
        );
    }

    /// A double quote inside a `{{-- ... --}}` comment must not be mistaken
    /// for the start of a PHP string literal either — same root cause as
    /// the apostrophe case above.
    #[test]
    fn test_preprocess_comment_with_double_quote_does_not_desync() {
        let content = "{{-- say \"hi\" --}}\n<p>{{ $after }}</p>\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("/*  say \"hi\""),
            "comment should translate to a block comment: {}",
            php
        );
        assert!(
            php.contains("echo e( $after )"),
            "content after the comment should still translate normally: {}",
            php
        );
    }

    /// The text of the first `/* ... */` block comment in the virtual PHP.
    /// Panics if there is no closed block comment, which is itself the bug
    /// the callers are guarding against.
    fn comment_body(php: &str) -> &str {
        let start = php.find("/* ").expect("a block comment should be emitted");
        let rest = &php[start + 3..];
        let end = rest.find("*/").expect("the comment should be closed");
        &rest[..end]
    }

    /// Commenting out an echo is the usual reason to write a Blade comment,
    /// so the `}}` / `!!}` of the commented-out echo must not be taken for
    /// the comment's terminator: only a contiguous `--}}` ends a comment.
    #[test]
    fn test_preprocess_comment_containing_echo_does_not_desync() {
        for content in [
            "{{-- {{ $old }} --}}\n<p>{{ $after }}</p>\n",
            "{{-- {!! $old !!} --}}\n<p>{{ $after }}</p>\n",
        ] {
            let (php, _) = preprocess(content);
            assert!(
                comment_body(&php).contains("$old"),
                "the commented-out echo should stay inside the block comment: {}",
                php
            );
            assert!(
                php.contains("echo e( $after )"),
                "content after the comment should still translate normally: {}",
                php
            );
        }
    }

    /// `@endphp` mentioned in comment prose is text, not the end of an
    /// `@php` block, so it must not terminate the comment either.
    #[test]
    fn test_preprocess_comment_mentioning_endphp_does_not_desync() {
        let content = "{{-- use @php/@endphp instead --}}\n<p>{{ $after }}</p>\n";
        let (php, _) = preprocess(content);
        assert!(
            comment_body(&php).contains("@endphp instead"),
            "the mentioned directive should stay inside the block comment: {}",
            php
        );
        assert!(
            php.contains("echo e( $after )"),
            "content after the comment should still translate normally: {}",
            php
        );
    }

    /// Commenting out a block of PHP is the usual reason to write a Blade
    /// comment, so a `*/` in the comment text must not close the emitted
    /// block comment early — everything after it would become live PHP.
    #[test]
    fn test_preprocess_comment_containing_block_comment_end_does_not_desync() {
        let content = "{{-- see /* legacy */ code --}}\n<p>{{ $after }}</p>\n";
        let (php, _) = preprocess(content);
        let body = comment_body(&php);
        assert!(
            body.contains("legacy") && body.contains("code"),
            "the whole comment text should stay inside the block comment: {}",
            php
        );
        assert!(
            php.contains("echo e( $after )"),
            "content after the comment should still translate normally: {}",
            php
        );
        let emitted = php
            .lines()
            .find(|l| l.contains("legacy"))
            .expect("the comment line");
        assert_eq!(
            emitted.encode_utf16().count(),
            content.lines().next().unwrap().encode_utf16().count() + 2,
            "blanking `*/` must keep the columns aligned; only the \
             two-character `--}}` terminator grows (to ` */ `): {}",
            php
        );
    }

    /// An unterminated `{{--` must still emit a closed `/* ... */`, or the
    /// open comment swallows the wrapper function's closing brace and makes
    /// the whole virtual file unparseable.
    #[test]
    fn test_preprocess_unterminated_comment_is_closed() {
        let content = "<p>{{ $before }}</p>\n{{-- forgot to close\nstill comment\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("echo e( $before )"),
            "content before the comment should translate normally: {}",
            php
        );
        let comment_start = php.find("/* ").expect("comment should be emitted");
        let comment_end = php[comment_start..]
            .find("*/")
            .expect("unterminated comment should still be closed");
        assert!(
            php[comment_start + comment_end..].contains('}'),
            "the wrapper function's closing brace must not be inside the comment: {}",
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

    /// The prologue text before the wrapper function: where every declared
    /// variable lives.
    fn prologue_of(php: &str) -> &str {
        php.split_once("function __blade_template()").unwrap().0
    }

    /// `@props` declares each key in the prologue, typed from its default
    /// value, so the forward walker sees it as defined and typed without
    /// waiting on the caller's `<x-… />` attributes.
    #[test]
    fn test_preprocess_props_directive_declares_variables() {
        let content = "@props(['caption' => '', 'count' => 0])\n{{ $caption }}\n";
        let (php, _) = preprocess(content);
        let prologue = prologue_of(&php);
        assert!(
            prologue.contains("$caption = '';") && prologue.contains("$count = 0;"),
            "@props should declare each key with its default: {}",
            php
        );
        assert!(
            php.contains("global $errors, $__env, $caption, $count;"),
            "props must be pulled into the wrapper scope: {}",
            php
        );
    }

    /// The declaration belongs in the prologue, not the template body. An
    /// assignment in the body would overwrite whatever type the author
    /// declared for the same name, and read as a dead local assignment to
    /// the unused-variable check.
    #[test]
    fn test_preprocess_props_directive_does_not_assign_in_the_body() {
        let content = "@props(['caption' => ''])\n<span>{{ $caption }}</span>\n";
        let (php, _) = preprocess(content);
        let body = php.split_once("function __blade_template()").unwrap().1;
        assert!(
            !body.contains("$caption ="),
            "the body must not re-assign a prop: {}",
            php
        );
        // The default expression stays visible so it is still type-checked.
        assert!(
            body.contains("blade_directive"),
            "the directive's arguments should still be analysed: {}",
            php
        );
    }

    /// A `@props` key the template's own docblock already declares keeps the
    /// declared type: the signature is the contract, `@props` only supplies
    /// what the signature leaves out.
    #[test]
    fn test_declared_signature_wins_over_a_props_default() {
        let content = "@php\n/**\n * @var \\App\\Options $options\n */\n@endphp\n@props(['options' => []])\n{{ $options->first() }}\n";
        let (php, _) = preprocess(content);
        assert!(
            !php.contains("$options = [];"),
            "the props default must not shadow the declared type: {}",
            php
        );
    }

    /// The array literal in `@props(...)` commonly spans multiple lines;
    /// the whole argument list must be read, not just the closing line, or
    /// every prop declared before the last line is lost.
    #[test]
    fn test_preprocess_props_directive_spans_multiple_lines() {
        let content = "@props([\n    'caption' => '',\n])\n{{ $caption }}\n";
        let (php, _) = preprocess(content);
        assert!(
            prologue_of(&php).contains("$caption = '';"),
            "a multi-line @props array must still declare its keys: {}",
            php
        );
    }

    /// A prop with no default (`@props(['visible'])`) is *required*: its
    /// value comes from the caller, so it is declared `mixed` rather than
    /// being invented as `null`, which would make every use of it a type
    /// error against whatever the prop is really passed.
    #[test]
    fn test_preprocess_props_directive_shorthand_without_default() {
        let content = "@props(['visible'])\n{{ $visible }}\n";
        let (php, _) = preprocess(content);
        assert!(
            prologue_of(&php).contains("/** @var mixed $visible */"),
            "a defaultless prop should be declared mixed: {}",
            php
        );
    }

    /// `@aware` pulls a value from the parent component, falling back to the
    /// declared default, so it types the body exactly as `@props` does.
    #[test]
    fn test_preprocess_aware_directive_declares_variables() {
        let content = "@aware(['color' => 'gray'])\n{{ $color }}\n";
        let (php, _) = preprocess(content);
        assert!(
            prologue_of(&php).contains("$color = 'gray';"),
            "@aware should declare its keys: {}",
            php
        );
    }

    /// A dynamic props argument (not a plain array literal) cannot be read,
    /// so no variable is invented; the expression still reaches PHP as an
    /// inert call so its own variables are seen.
    #[test]
    fn test_preprocess_props_directive_dynamic_argument_falls_back() {
        let content = "@props($dynamicProps)\n";
        let (php, _) = preprocess(content);
        assert!(
            php.contains("blade_directive ($dynamicProps);"),
            "a non-literal @props argument should fall back to the inert call: {}",
            php
        );
        assert!(
            php.contains("global $errors, $__env;"),
            "a non-literal @props argument declares nothing: {}",
            php
        );
    }
}
