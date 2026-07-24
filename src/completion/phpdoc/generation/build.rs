//! Building the actual docblock text (snippet and plain-text forms)
//! from a classified declaration, plus the type-enrichment helpers
//! that decide when a native type hint needs a PHPDoc tag at all.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::Position;

use crate::completion::phpdoc::context::{DocblockContext, SymbolInfo};
use crate::completion::resolver::FunctionLoaderFn;
use crate::completion::source::comment_position::position_to_byte_offset;
use crate::completion::source::throws_analysis::{self, ThrowsContext};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionLoader};

// ─── Type Enrichment Helpers ────────────────────────────────────────────────

/// Check whether a `PhpType` is a bare callable/Closure keyword (no signature).
fn is_callable_keyword(pt: &PhpType) -> bool {
    pt.is_callable()
}

/// Check whether a `PhpType` is a bare `array` keyword (no generic params).
fn is_bare_array(pt: &PhpType) -> bool {
    pt.is_bare_array()
}

/// Extract the callable display name from a `PhpType` that satisfies
/// `is_callable_keyword`.
fn callable_display_name(pt: &PhpType) -> &str {
    match pt {
        PhpType::Named(s) => s.as_str(),
        _ => "callable",
    }
}

/// Determine whether a native type hint "needs enrichment" via a PHPDoc
/// tag, and if so return the tag type string to use.
///
/// Returns `None` when the native type is fully expressed (scalars,
/// union types, intersection types, non-generic classes).
///
/// Returns `Some(tag_text)` when a PHPDoc tag should be emitted:
/// - Missing type → `"${N:mixed}"` (snippet) or `"mixed"` (plain)
/// - `array` → `"${N:array}"` (snippet) or `"array"` (plain)
/// - Class with templates → `"ClassName<${N:T1}, ${N+1:T2}>"` or plain equivalent
pub(crate) fn enrichment_snippet(
    type_hint: Option<&PhpType>,
    tab_stop: &mut u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let pt = match type_hint {
        None => {
            let s = format!("${{{}:mixed}}", *tab_stop);
            *tab_stop += 1;
            return Some(s);
        }
        Some(t) => t,
    };

    // `void` is never enriched for return types (caller handles skip).
    // `array` always needs enrichment.
    if is_bare_array(pt) {
        let s = format!("array<${{{}:mixed}}>", *tab_stop);
        *tab_stop += 1;
        return Some(s);
    }

    // `Closure` / `callable` need a callable-signature placeholder.
    if is_callable_keyword(pt) {
        let name = callable_display_name(pt);
        let s = format!("({}(): ${{{}:mixed}})", name, *tab_stop);
        *tab_stop += 1;
        return Some(s);
    }

    // Union types — enrich individual callable / array parts.
    // Use union_members to correctly handle generic nesting
    // (e.g. `Collection<int|string, User>|null` must not be split on the inner `|`).
    let members = pt.union_members();
    if members.len() > 1 {
        let needs = members
            .iter()
            .any(|member| is_bare_array(member) || is_callable_keyword(member));
        if needs {
            let enriched_parts: Vec<String> = members
                .iter()
                .map(|member| {
                    if is_callable_keyword(member) {
                        let name = callable_display_name(member);
                        format!("({}(): ${{{}:mixed}})", name, {
                            let t = *tab_stop;
                            *tab_stop += 1;
                            t
                        })
                    } else if is_bare_array(member) {
                        let s = format!("array<${{{}:mixed}}>", *tab_stop);
                        *tab_stop += 1;
                        s
                    } else {
                        member.to_string()
                    }
                })
                .collect();
            return Some(enriched_parts.join("|"));
        }
        return None;
    }

    // Intersection types (&), nullable (?Type) — skip.
    if matches!(pt, PhpType::Intersection(_) | PhpType::Nullable(_)) {
        return None;
    }

    // Scalar / built-in types never have template parameters.
    if pt.is_scalar() {
        return None;
    }

    // Try to load the class and check for templates.
    if let Some(name) = pt.base_name()
        && let Some(cls) = class_loader(name)
        && !cls.template_params.is_empty()
    {
        let mut parts = Vec::new();
        for tp in &cls.template_params {
            parts.push(format!("${{{}:{}}}", *tab_stop, tp));
            *tab_stop += 1;
        }
        return Some(format!("{}<{}>", name, parts.join(", ")));
    }

    None
}

/// Structured version of [`enrichment_plain`] returning a [`PhpType`]
/// instead of a display string.
///
/// Use this when the enriched type needs to be compared structurally
/// (via [`PhpType::equivalent`]) rather than by string equality. The
/// plain-text callers that only need a display string should keep using
/// [`enrichment_plain`].
pub(crate) fn enrichment_plain_typed(
    type_hint: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    let pt = match type_hint {
        None => return Some(PhpType::mixed()),
        Some(t) => t,
    };

    if is_bare_array(pt) {
        return Some(PhpType::generic_array_val(PhpType::mixed()));
    }

    if is_callable_keyword(pt) {
        let kind = callable_display_name(pt).to_string();
        return Some(PhpType::Callable {
            kind,
            params: vec![],
            return_type: Some(Box::new(PhpType::mixed())),
        });
    }

    // Union types — enrich individual callable / array parts.
    let members = pt.union_members();
    if members.len() > 1 {
        let needs = members
            .iter()
            .any(|member| is_bare_array(member) || is_callable_keyword(member));
        if needs {
            let enriched: Vec<PhpType> = members
                .iter()
                .map(|member| {
                    if is_callable_keyword(member) {
                        let kind = callable_display_name(member).to_string();
                        PhpType::Callable {
                            kind,
                            params: vec![],
                            return_type: Some(Box::new(PhpType::mixed())),
                        }
                    } else if is_bare_array(member) {
                        PhpType::generic_array_val(PhpType::mixed())
                    } else {
                        (*member).clone()
                    }
                })
                .collect();
            return Some(PhpType::Union(enriched));
        }
        return None;
    }

    if matches!(pt, PhpType::Intersection(_) | PhpType::Nullable(_)) {
        return None;
    }

    // Scalar / built-in types never have template parameters.
    if pt.is_scalar() {
        return None;
    }

    // Try to load the class and check for templates.
    if let Some(name) = pt.base_name()
        && let Some(cls) = class_loader(name)
        && !cls.template_params.is_empty()
    {
        let args: Vec<PhpType> = cls
            .template_params
            .iter()
            .map(|s| PhpType::Named(s.to_string()))
            .collect();
        return Some(PhpType::Generic(name.to_string(), args));
    }

    None
}

/// Plain-text version of `enrichment_snippet` (no tab stops).
///
/// Also used by tag completion (`build_phpdoc_completions`) to enrich
/// `@param`, `@return`, and `@var` type hints with template parameters.
///
/// Callable types are wrapped in parentheses for PHPDoc notation:
/// `(Closure(): mixed)`, `(callable(): mixed)`.
pub(crate) fn enrichment_plain(
    type_hint: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<String> {
    let typed = enrichment_plain_typed(type_hint, class_loader)?;

    // Callable types need parentheses in PHPDoc notation, which
    // PhpType::Display does not add.
    Some(enrichment_type_to_plain(&typed))
}

/// Format an enriched `PhpType` as a plain-text PHPDoc type string.
///
/// Callable types are wrapped in parentheses (`(Closure(): mixed)`)
/// to match PHPDoc inline callable notation. Union members are
/// formatted individually and joined with `|`.
fn enrichment_type_to_plain(ty: &PhpType) -> String {
    match ty {
        PhpType::Callable { .. } => format!("({})", ty),
        PhpType::Union(members) => members
            .iter()
            .map(enrichment_type_to_plain)
            .collect::<Vec<_>>()
            .join("|"),
        _ => ty.to_string(),
    }
}

// ─── Snippet / Plain Builder ────────────────────────────────────────────────

/// Build the full docblock as plain text (no tab stops).
///
/// Used by the `onTypeFormatting` path where snippets are not supported.
///
/// Only called for declaration-level contexts (`FunctionOrMethod`,
/// `ClassLike`, `Property`, `Constant`).  `Inline` and `Unknown` are
/// filtered out by the caller before we get here.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_docblock_plain(
    context: &DocblockContext,
    sym: &SymbolInfo,
    indent: &str,
    content: &str,
    position: Position,
    _use_map: &HashMap<String, String>,
    _file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> String {
    match context {
        DocblockContext::FunctionOrMethod => build_function_plain(
            sym,
            indent,
            content,
            position,
            _use_map,
            _file_namespace,
            local_classes,
            class_loader,
            function_loader,
        ),
        DocblockContext::ClassLike => build_class_plain(sym, indent, class_loader),
        DocblockContext::Property => build_property_plain(sym, indent, class_loader),
        DocblockContext::Constant => build_constant_plain(sym, indent, class_loader),
        // Inline and Unknown are early-returned by the caller.
        DocblockContext::Inline | DocblockContext::Unknown => String::new(),
    }
}

/// Build the full docblock snippet text.
///
/// The snippet uses VSCode-style tab stops (`$1`, `$2`, etc.) so the
/// user can tab through the placeholders.
///
/// Only called for declaration-level contexts (`FunctionOrMethod`,
/// `ClassLike`, `Property`, `Constant`).  `Inline` and `Unknown` are
/// filtered out by the caller before we get here.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_docblock_snippet(
    context: &DocblockContext,
    sym: &SymbolInfo,
    indent: &str,
    content: &str,
    position: Position,
    _use_map: &HashMap<String, String>,
    _file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> String {
    match context {
        DocblockContext::FunctionOrMethod => build_function_snippet(
            sym,
            indent,
            content,
            position,
            _use_map,
            _file_namespace,
            local_classes,
            class_loader,
            function_loader,
        ),
        DocblockContext::ClassLike => build_class_snippet(sym, indent, class_loader),
        DocblockContext::Property => build_property_snippet(sym, indent, class_loader),
        DocblockContext::Constant => build_constant_snippet(sym, indent, class_loader),
        // Inline and Unknown are early-returned by the caller.
        DocblockContext::Inline | DocblockContext::Unknown => String::new(),
    }
}

/// Build a docblock snippet for a function or method.
///
/// Only emits `@param` / `@return` tags when the native type needs
/// enrichment (missing, `array`, or class with `@template` params).
/// `@throws` tags are always emitted for uncaught exceptions.
/// Tags are grouped with blank `*` lines between groups.
/// Parameter names within the `@param` block are space-aligned.
#[allow(clippy::too_many_arguments)]
fn build_function_snippet(
    sym: &SymbolInfo,
    _indent: &str,
    content: &str,
    position: Position,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> String {
    let throws_ctx = ThrowsContext {
        class_loader,
        function_loader,
        use_map,
        file_namespace,
    };
    let uncaught = throws_analysis::find_uncaught_throw_types_with_context(
        content,
        position,
        Some(&throws_ctx),
    );

    let mut tab_stop = 1u32;

    // Collect @param tags that need enrichment.
    // Each entry is (snippet_type, display_len, escaped_name).
    let mut param_tags: Vec<(String, usize, String)> = Vec::new();
    for (type_hint, name) in &sym.params {
        if let Some(enriched) = enrichment_snippet(type_hint.as_ref(), &mut tab_stop, class_loader)
        {
            // Use the plain-text version to measure the rendered width for
            // alignment.  The snippet version contains `${N:...}` markers
            // that inflate its length.
            let display_len = enrichment_plain(type_hint.as_ref(), class_loader)
                .map(|p| p.len())
                .unwrap_or(enriched.len());
            // Escape `$` in PHP parameter names so the snippet parser
            // does not treat them as snippet variables.
            param_tags.push((enriched, display_len, name.replace('$', "\\$")));
        }
    }

    // Determine @return enrichment.
    // Constructors never get @return (they implicitly return the class).
    let is_constructor = sym
        .method_name
        .as_ref()
        .is_some_and(|n| n.eq_ignore_ascii_case("__construct"));
    let is_void = sym.return_type.as_ref().is_some_and(|r| r.is_void());
    let return_tag = if is_void || is_constructor {
        None
    } else {
        // Try body-based inference first (produces richer types like
        // `list<string>` instead of `array<mixed>`).
        let body_inferred = crate::code_actions::phpstan::fix_return_type::enrichment_return_type(
            content,
            position,
            local_classes,
            class_loader,
            function_loader,
        );
        let inferred = body_inferred.filter(|t| {
            !t.is_void()
                && !t.is_mixed()
                && sym.return_type.as_ref().is_none_or(|s| !t.equivalent(s))
        });
        // Fall back to signature-based enrichment when body inference
        // doesn't produce anything useful.
        if let Some(t) = inferred {
            Some(t.to_string())
        } else {
            enrichment_snippet(sym.return_type.as_ref(), &mut tab_stop, class_loader)
        }
    };

    let has_throws = !uncaught.is_empty();

    let has_any_tag = !param_tags.is_empty() || has_throws || return_tag.is_some();

    let mut lines = Vec::new();
    lines.push("/**".to_string());

    if !has_any_tag {
        // No tags — emit a summary-only skeleton.
        lines.push(" * ${1}".to_string());
    }

    // @param block with space-aligned names.
    if !param_tags.is_empty() {
        let max_display_len = param_tags.iter().map(|(_, dl, _)| *dl).max().unwrap_or(0);
        for (type_str, display_len, name) in &param_tags {
            let padding = " ".repeat(max_display_len - display_len);
            lines.push(format!(" * @param {}{} {}", type_str, padding, name));
        }
    }

    // @throws block (blank separator from preceding group).
    if has_throws {
        if !param_tags.is_empty() {
            lines.push(" *".to_string());
        }
        for exc in &uncaught {
            let exc_str = exc.to_string();
            let display = crate::util::short_name(&exc_str);
            lines.push(format!(" * @throws {}", display));
        }
    }

    // @return tag (blank separator from preceding group).
    if let Some(ret) = return_tag {
        if !param_tags.is_empty() || has_throws {
            lines.push(" *".to_string());
        }
        lines.push(format!(" * @return {}", ret));
    }

    lines.push(" */".to_string());
    lines.join("\n")
}

/// Build a plain-text docblock for a function or method (no tab stops).
///
/// Same enrichment logic as the snippet builder, but without tab stops.
#[allow(clippy::too_many_arguments)]
fn build_function_plain(
    sym: &SymbolInfo,
    indent: &str,
    content: &str,
    position: Position,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
) -> String {
    let throws_ctx = ThrowsContext {
        class_loader,
        function_loader,
        use_map,
        file_namespace,
    };
    let uncaught = throws_analysis::find_uncaught_throw_types_with_context(
        content,
        position,
        Some(&throws_ctx),
    );

    // Collect @param tags that need enrichment.
    let mut param_tags: Vec<(String, String)> = Vec::new();
    for (type_hint, name) in &sym.params {
        if let Some(enriched) = enrichment_plain(type_hint.as_ref(), class_loader) {
            param_tags.push((enriched, name.clone()));
        }
    }

    // Constructors never get @return.
    let is_constructor = sym
        .method_name
        .as_ref()
        .is_some_and(|n| n.eq_ignore_ascii_case("__construct"));
    let is_void = sym.return_type.as_ref().is_some_and(|r| r.is_void());
    let return_tag = if is_void || is_constructor {
        None
    } else {
        // Try body-based inference first (produces richer types like
        // `list<string>` instead of `array<mixed>`).
        let body_inferred = crate::code_actions::phpstan::fix_return_type::enrichment_return_type(
            content,
            position,
            local_classes,
            class_loader,
            function_loader,
        );
        // Filter out types that don't need a @return tag (void, scalars
        // that match the native hint exactly).
        let inferred = body_inferred.filter(|t| {
            !t.is_void()
                && !t.is_mixed()
                && sym.return_type.as_ref().is_none_or(|s| !t.equivalent(s))
        });
        // Fall back to signature-based enrichment when body inference
        // doesn't produce anything useful.
        inferred
            .map(|t| t.to_string())
            .or_else(|| enrichment_plain(sym.return_type.as_ref(), class_loader))
    };

    let has_throws = !uncaught.is_empty();

    let has_any_tag = !param_tags.is_empty() || has_throws || return_tag.is_some();

    let mut lines = Vec::new();
    lines.push(format!("{}/**", indent));

    if !has_any_tag {
        lines.push(format!("{} * ", indent));
    }

    if !param_tags.is_empty() {
        let max_type_len = param_tags.iter().map(|(t, _)| t.len()).max().unwrap_or(0);
        for (type_str, name) in &param_tags {
            let padding = " ".repeat(max_type_len - type_str.len());
            lines.push(format!(
                "{} * @param {}{} {}",
                indent, type_str, padding, name
            ));
        }
    }

    if has_throws {
        if !param_tags.is_empty() {
            lines.push(format!("{} *", indent));
        }
        for exc in &uncaught {
            let exc_str = exc.to_string();
            let display = crate::util::short_name(&exc_str).to_string();
            lines.push(format!("{} * @throws {}", indent, display));
        }
    }

    if let Some(ret) = return_tag {
        if !param_tags.is_empty() || has_throws {
            lines.push(format!("{} *", indent));
        }
        lines.push(format!("{} * @return {}", indent, ret));
    }

    lines.push(format!("{} */", indent));
    lines.join("\n") + "\n"
}

/// Build a plain-text docblock for a class (no tab stops).
///
/// Generates `@extends` / `@implements` tags when the parent or
/// interface has `@template` parameters.
fn build_class_plain(
    sym: &SymbolInfo,
    indent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let mut tag_lines = Vec::new();

    for parent in &sym.extends_names {
        if let Some(cls) = class_loader(parent)
            && !cls.template_params.is_empty()
        {
            let parts: Vec<&str> = cls.template_params.iter().map(|s| s.as_str()).collect();
            tag_lines.push(format!(
                "{} * @extends {}<{}>",
                indent,
                parent,
                parts.join(", ")
            ));
        }
    }

    for iface in &sym.implements_names {
        if let Some(cls) = class_loader(iface)
            && !cls.template_params.is_empty()
        {
            let parts: Vec<&str> = cls.template_params.iter().map(|s| s.as_str()).collect();
            tag_lines.push(format!(
                "{} * @implements {}<{}>",
                indent,
                iface,
                parts.join(", ")
            ));
        }
    }

    if tag_lines.is_empty() {
        format!("{indent}/**\n{indent} * \n{indent} */\n")
    } else {
        let mut lines = Vec::new();
        lines.push(format!("{}/**", indent));
        lines.extend(tag_lines);
        lines.push(format!("{} */", indent));
        lines.join("\n") + "\n"
    }
}

/// Build a plain-text docblock for a property (no tab stops).
///
/// Emits a single-line `/** @var Type */` format.
fn build_property_plain(
    sym: &SymbolInfo,
    indent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let var_type = property_var_type_plain(sym.type_hint.as_ref(), class_loader);
    format!("{indent}/** @var {var_type} */\n")
}

/// Build a plain-text docblock for a constant (no tab stops).
///
/// Emits a single-line `/** @var Type */` format.
fn build_constant_plain(
    sym: &SymbolInfo,
    indent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let var_type = property_var_type_plain(sym.type_hint.as_ref(), class_loader);
    format!("{indent}/** @var {var_type} */\n")
}

/// Build a docblock snippet for a class, interface, trait, or enum.
///
/// Generates `@extends` / `@implements` tags with tab-stop placeholders
/// when the parent or interface has `@template` parameters.
fn build_class_snippet(
    sym: &SymbolInfo,
    _indent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let mut tag_lines = Vec::new();
    let mut tab_stop = 1u32;

    for parent in &sym.extends_names {
        if let Some(cls) = class_loader(parent)
            && !cls.template_params.is_empty()
        {
            let mut parts = Vec::new();
            for tp in &cls.template_params {
                parts.push(format!("${{{}:{}}}", tab_stop, tp));
                tab_stop += 1;
            }
            tag_lines.push(format!(" * @extends {}<{}>", parent, parts.join(", ")));
        }
    }

    for iface in &sym.implements_names {
        if let Some(cls) = class_loader(iface)
            && !cls.template_params.is_empty()
        {
            let mut parts = Vec::new();
            for tp in &cls.template_params {
                parts.push(format!("${{{}:{}}}", tab_stop, tp));
                tab_stop += 1;
            }
            tag_lines.push(format!(" * @implements {}<{}>", iface, parts.join(", ")));
        }
    }

    let mut lines = Vec::new();
    lines.push("/**".to_string());

    if tag_lines.is_empty() {
        // No template tags — emit a summary-only skeleton.
        lines.push(" * ${1}".to_string());
    } else {
        lines.extend(tag_lines);
    }

    lines.push(" */".to_string());
    lines.join("\n")
}

/// Build a docblock snippet for a property.
///
/// Emits a single-line `/** @var Type */` format.
/// For missing types, the type is a tab-stop placeholder.
/// For classes with templates, template names are tab-stop placeholders.
fn build_property_snippet(
    sym: &SymbolInfo,
    _indent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let mut tab_stop = 1u32;
    let var_type = property_var_type_snippet(sym.type_hint.as_ref(), &mut tab_stop, class_loader);
    format!("/** @var {} */", var_type)
}

/// Build a docblock snippet for a constant.
///
/// Emits a single-line `/** @var Type */` format.
fn build_constant_snippet(
    sym: &SymbolInfo,
    _indent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let mut tab_stop = 1u32;
    let var_type = property_var_type_snippet(sym.type_hint.as_ref(), &mut tab_stop, class_loader);
    format!("/** @var {} */", var_type)
}

/// Attempt to infer the type of an inline variable assignment using the
/// hover type-resolution pipeline.
///
/// Given `$var = ['']`, this resolves to `list<string>` by delegating
/// to the same `resolve_variable_type` that powers hover.
pub(crate) fn infer_inline_variable_type(
    sym: &SymbolInfo,
    content: &str,
    position: Position,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoaderFn<'_>,
) -> Option<PhpType> {
    let var_name = sym.variable_name.as_deref()?;

    // The cursor is at the `/**` trigger, which is above the variable
    // assignment.  We need an offset that falls within the assignment
    // line so that the resolution pipeline can find the assignment.
    let trigger_offset = position_to_byte_offset(content, position);

    // The `/**` trigger may be unclosed (completion path) or already
    // closed as `/** */` (on-enter path).  An unclosed `/**` causes
    // the PHP parser to swallow the assignment line into a comment,
    // making it invisible to the AST.  Fix this by replacing the
    // docblock trigger text with spaces so the parser sees the
    // assignment.
    let patched = patch_docblock_trigger(content, trigger_offset);
    let effective_content = patched.as_deref().unwrap_or(content);

    // Place the cursor after the assignment's semicolon so the
    // resolution pipeline (which scans backwards) can find it.
    let cursor_offset = effective_content[trigger_offset..]
        .find(';')
        .map(|off| trigger_offset + off + 1)
        .unwrap_or(trigger_offset + 1) as u32;

    let current_class = crate::class_lookup::find_class_at_offset(all_classes, cursor_offset);

    crate::completion::variable::resolution::resolve_variable_php_type(
        var_name,
        effective_content,
        cursor_offset,
        current_class,
        all_classes,
        class_loader,
        crate::completion::resolver::Loaders::with_function(function_loader),
    )
}

/// Replace the `/**` (or `/** */`) block around `trigger_offset` with
/// spaces so the PHP parser does not swallow the next line into a
/// docblock comment.
///
/// Returns `None` when no patching is needed (no `/**` found).
fn patch_docblock_trigger(content: &str, trigger_offset: usize) -> Option<String> {
    // Walk backwards from the trigger to find the start of `/**`.
    let before = &content[..trigger_offset];
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let trigger_line = &content[line_start..];

    // Find the `/**` on this line.
    let doc_start_in_line = trigger_line.find("/**")?;
    let abs_doc_start = line_start + doc_start_in_line;

    // Find the end of the docblock: either `*/` on the same or next
    // lines, or end-of-line if unclosed.
    let after_open = abs_doc_start + 3; // skip `/**`
    let abs_doc_end = if let Some(close) = content[after_open..].find("*/") {
        after_open + close + 2
    } else {
        // Unclosed — blank out to end of the line containing `/**`.
        content[abs_doc_start..]
            .find('\n')
            .map(|i| abs_doc_start + i)
            .unwrap_or(content.len())
    };

    let mut patched = content.to_string();
    // Replace the docblock region with spaces (preserving byte offsets).
    patched.replace_range(
        abs_doc_start..abs_doc_end,
        &" ".repeat(abs_doc_end - abs_doc_start),
    );
    Some(patched)
}

/// Compute the `@var` type string for a property/constant snippet.
///
/// - Missing type → `${N:mixed}` tab stop
/// - `array` → `${N:array}` tab stop
/// - Class with templates → `ClassName<${N:T1}, ...>` tab stops
/// - Other → literal type string
fn property_var_type_snippet(
    type_hint: Option<&PhpType>,
    tab_stop: &mut u32,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    match type_hint {
        None => {
            let s = format!("${{{}:mixed}}", *tab_stop);
            *tab_stop += 1;
            s
        }
        Some(th) if th.is_bare_array() => {
            let s = format!("${{{}:array}}", *tab_stop);
            *tab_stop += 1;
            s
        }
        Some(th) => {
            let shortened = th.shorten();
            // Callable types get a signature placeholder.
            if th.is_callable() {
                let s = format!("(${{{}:{}()}})", *tab_stop, shortened);
                *tab_stop += 1;
                return s;
            }
            if let Some(name) = shortened.base_name()
                && let Some(cls) = class_loader(name)
                && !cls.template_params.is_empty()
            {
                let mut parts = Vec::new();
                for tp in &cls.template_params {
                    parts.push(format!("${{{}:{}}}", *tab_stop, tp));
                    *tab_stop += 1;
                }
                return format!("{}<{}>", name, parts.join(", "));
            }
            shortened.to_string()
        }
    }
}

/// Compute the `@var` type string for a property/constant in plain text.
fn property_var_type_plain(
    type_hint: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    match type_hint {
        None => PhpType::mixed().to_string(),
        Some(th) if th.is_bare_array() => "array".to_string(),
        Some(th) => {
            let shortened = th.shorten();
            if th.is_callable() {
                return format!("({}())", shortened);
            }
            if let Some(name) = shortened.base_name()
                && let Some(cls) = class_loader(name)
                && !cls.template_params.is_empty()
            {
                let parts: Vec<&str> = cls.template_params.iter().map(|s| s.as_str()).collect();
                return format!("{}<{}>", name, parts.join(", "));
            }
            shortened.to_string()
        }
    }
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
