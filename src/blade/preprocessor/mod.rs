use super::TemplateKind;
use super::directives::{
    CUSTOM_MARKER, CustomDirectives, CustomForm, match_directive, translate_directive,
};
use super::source_map::BladeSourceMap;

mod component_call;
#[cfg(test)]
mod tests;

pub use component_call::ARGUMENT_VAR_PREFIX;

use component_call::{
    OpenComponentCall, bound_attr_open_len, bound_attr_spans_lines, component_tag_at, contains_seq,
};

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
    // The marker functions the lowering calls are declared once for the
    // whole project, as a stub (see `blade::with_marker_stubs`), rather
    // than by every template that calls them.
    virtual_php.push_str("<?php\n");
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

    // A directive appearing while an echo or escaped echo is still open
    // ends it. Blade's statement compiler runs before the echo compiler, so
    // by the time echo compilation would see a directive, Blade has already
    // turned it into real PHP (or, for an escaped echo, the directive was
    // never part of the frontend-only text to begin with). Absorbing the
    // directive as part of the echo instead leaves whatever block it closes
    // (`@endif`, `@endforeach`, ...) unclosed in the emitted PHP.
    let directive_boundary = |remaining: &[char]| -> bool {
        remaining.first() == Some(&'@') && {
            let rest: String = remaining[1..].iter().collect();
            match_directive(&rest).is_some() || custom_directives.match_directive(&rest).is_some()
        }
    };

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
                    echo_closes_at_eol = if raw {
                        !contains_seq(&remaining[4..], &['!', '!', '}'])
                            && last_raw_echo_close.is_none_or(|last| last <= line_idx)
                    } else {
                        !contains_seq(&remaining[3..], &['}', '}'])
                            && last_escaped_echo_close.is_none_or(|last| last <= line_idx)
                    };
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
                } else if directive_boundary(remaining) {
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
                } else if !in_php_directive_block && directive_boundary(remaining) {
                    replacement = if raw_echo {
                        "; ".to_string()
                    } else {
                        "); ".to_string()
                    };
                    next_mode = Mode::Html;
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
