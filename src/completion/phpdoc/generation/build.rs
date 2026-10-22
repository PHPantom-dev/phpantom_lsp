//! Building the actual docblock text (snippet and plain-text forms)
//! from a classified declaration, plus the type-enrichment helpers
//! that decide when a native type hint needs a PHPDoc tag at all.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::Position;

use crate::class_lookup::{ResolvedClassLoader, SourceClassLoader};
use crate::completion::phpdoc::context::{DocblockContext, SymbolInfo};
use crate::completion::source::comment_position::position_to_byte_offset;
use crate::completion::source::throws_analysis::{self, ThrowsContext};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::{FileCtx, FunctionLoaderFn};
use crate::type_engine::variable::resolution::VarQuery;
use crate::types::{ClassInfo, FunctionLoader};

// ─── Type Enrichment Helpers ────────────────────────────────────────────────

/// Extract the callable display name from a `PhpType` that satisfies
/// [`PhpType::is_callable`].
fn callable_display_name(pt: &PhpType) -> &str {
    match pt.kind() {
        TypeKind::Named(s) => s.as_str(),
        _ => "callable",
    }
}

/// Decide whether a native type hint "needs enrichment" via a PHPDoc tag,
/// and if so build the type the tag should carry.
///
/// Returns `None` when the native type is fully expressed (scalars,
/// nullable and intersection types, unions without a bare `array` or
/// callable member, non-generic classes). Otherwise:
/// - Missing type → `mixed`
/// - `array` → `array<mixed>`
/// - `Closure` / `callable` → `Closure(): mixed`
/// - a union → each bare `array` / callable member enriched as above
/// - a class with templates → `ClassName<T1, T2>`
///
/// Each part the user is expected to fill in (the `mixed` placeholders
/// and the template parameters) is built by `fill` from its default, so
/// the snippet form can put a tab stop there. Class names are written
/// through `shorten`.
fn enrich_type(
    type_hint: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    shorten: &dyn Fn(&str) -> String,
    fill: &mut dyn FnMut(&str) -> PhpType,
) -> Option<PhpType> {
    let Some(pt) = type_hint else {
        return Some(fill("mixed"));
    };

    // `void` is never enriched for return types (caller handles skip).
    if pt.is_bare_array() {
        return Some(PhpType::generic_array_val(fill("mixed")));
    }

    if pt.is_callable() {
        return Some(PhpType::callable_spec(
            callable_display_name(pt),
            vec![],
            Some(fill("mixed")),
        ));
    }

    // Union types — enrich individual callable / array parts.
    // `union_members` keeps generic nesting intact
    // (`Collection<int|string, User>|null` is not split on the inner `|`).
    let members = pt.union_members();
    if members.len() > 1 {
        if !members
            .iter()
            .any(|member| member.is_bare_array() || member.is_callable())
        {
            return None;
        }
        let enriched = members
            .iter()
            .map(|member| {
                if member.is_callable() {
                    PhpType::callable_spec(
                        callable_display_name(member),
                        vec![],
                        Some(fill("mixed")),
                    )
                } else if member.is_bare_array() {
                    PhpType::generic_array_val(fill("mixed"))
                } else {
                    member.resolve_names(shorten)
                }
            })
            .collect();
        return Some(PhpType::union(enriched));
    }

    if matches!(pt.kind(), TypeKind::Intersection(_) | TypeKind::Nullable(_)) {
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
        let args = cls.template_params.iter().map(|tp| fill(tp)).collect();
        return Some(PhpType::generic(shorten(name), args));
    }

    None
}

/// The enriched type for `type_hint` (see [`enrich_type`]) as snippet
/// text, with tab stops numbered from 1 over the parts the user is
/// expected to fill in.
pub(crate) fn enrichment_snippet(
    type_hint: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> Option<String> {
    DocForm::snippet().enrichment_text(type_hint, class_loader, use_map, file_namespace)
}

/// The enriched type for `type_hint` (see [`enrich_type`]) as a
/// [`PhpType`], with class names shortened for the file.
///
/// Use this when the enriched type needs to be compared structurally
/// (via [`PhpType::equivalent`]) rather than by string equality. The
/// plain-text callers that only need a display string should keep using
/// [`enrichment_plain`].
pub(crate) fn enrichment_plain_typed(
    type_hint: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> Option<PhpType> {
    enrich_type(
        type_hint,
        class_loader,
        &|name| crate::names::shorten_from_fqn(name, use_map, file_namespace),
        &mut |default| PhpType::parse(default),
    )
}

/// Plain-text version of [`enrichment_snippet`] (no tab stops).
///
/// Also used by tag completion (`build_phpdoc_completions`) to enrich
/// `@param`, `@return`, and `@var` type hints with template parameters.
///
/// Callable types are wrapped in parentheses for PHPDoc notation:
/// `(Closure(): mixed)`, `(callable(): mixed)`.
pub(crate) fn enrichment_plain(
    type_hint: Option<&PhpType>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> Option<String> {
    let typed = enrichment_plain_typed(type_hint, class_loader, use_map, file_namespace)?;
    Some(enrichment_type_to_plain(&typed))
}

/// Format an enriched `PhpType` as a PHPDoc type string.
///
/// A callable is wrapped in parentheses (`(Closure(): mixed)`) to match
/// PHPDoc inline callable notation, which `PhpType`'s `Display` only
/// does for a callable inside a union.
fn enrichment_type_to_plain(ty: &PhpType) -> String {
    match ty.kind() {
        TypeKind::Callable { .. } => format!("({})", ty),
        _ => ty.to_string(),
    }
}

// ─── Snippet / Plain Builder ────────────────────────────────────────────────

/// Which of its two forms a generated docblock takes.
pub(super) enum DocForm<'a> {
    /// An editor snippet, inserted at the cursor and so not indented, with
    /// tab stops over the parts the user is expected to fill in.
    Snippet { next_tab_stop: u32 },
    /// Plain text for the `onTypeFormatting` path, which takes no
    /// snippets: every line indented by `indent`, each ending in a newline.
    Plain { indent: &'a str },
}

impl DocForm<'_> {
    pub(super) fn snippet() -> Self {
        DocForm::Snippet { next_tab_stop: 1 }
    }

    /// A part the user is expected to fill in, showing `default`: the next
    /// tab stop in a snippet, `default` itself in plain text.
    fn field(&mut self, default: &str) -> String {
        match self {
            DocForm::Snippet { next_tab_stop } => {
                let n = *next_tab_stop;
                *next_tab_stop += 1;
                if default.is_empty() {
                    format!("${{{n}}}")
                } else {
                    format!("${{{n}:{default}}}")
                }
            }
            DocForm::Plain { .. } => default.to_string(),
        }
    }

    /// A parameter name, its `$` escaped in a snippet so the snippet
    /// parser does not read it as a variable.
    fn param_name(&self, name: &str) -> String {
        match self {
            DocForm::Snippet { .. } => name.replace('$', "\\$"),
            DocForm::Plain { .. } => name.to_string(),
        }
    }

    /// The enriched type for `type_hint` (see [`enrich_type`]) as text in
    /// this form, a tab stop over each part the user is expected to fill in.
    pub(super) fn enrichment_text(
        &mut self,
        type_hint: Option<&PhpType>,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<String> {
        let enriched = enrich_type(
            type_hint,
            class_loader,
            &|name| crate::names::shorten_from_fqn(name, use_map, file_namespace),
            &mut |default| PhpType::raw(self.field(default)),
        )?;
        Some(enrichment_type_to_plain(&enriched))
    }

    /// [`enrichment_text`](Self::enrichment_text) with the width it
    /// renders at, which aligns the `@param` names.
    fn enrich(
        &mut self,
        type_hint: Option<&PhpType>,
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
        use_map: &HashMap<String, String>,
        file_namespace: &Option<String>,
    ) -> Option<(String, usize)> {
        let text = self.enrichment_text(type_hint, class_loader, use_map, file_namespace)?;
        let width = match self {
            // The `${N:...}` markers inflate the snippet's length, so
            // measure the plain text it renders as.
            DocForm::Snippet { .. } => {
                enrichment_plain(type_hint, class_loader, use_map, file_namespace)
                    .map_or(text.len(), |plain| plain.len())
            }
            DocForm::Plain { .. } => text.len(),
        };
        Some((text, width))
    }

    /// The docblock made of `lines`.
    fn finish(&self, lines: &[String]) -> String {
        match self {
            DocForm::Snippet { .. } => lines.join("\n"),
            DocForm::Plain { indent } => lines
                .iter()
                .map(|line| format!("{indent}{line}\n"))
                .collect(),
        }
    }
}

/// Build the full docblock for a declaration, in `form`.
///
/// Only called for declaration-level contexts (`FunctionOrMethod`,
/// `ClassLike`, `Property`, `Constant`).  `Inline` and `Unknown` are
/// filtered out by the caller before we get here.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_docblock(
    context: &DocblockContext,
    sym: &SymbolInfo,
    form: &mut DocForm<'_>,
    content: &str,
    position: Position,
    decl_offset: u32,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &SourceClassLoader<'_>,
    resolved_class_loader: &ResolvedClassLoader<'_>,
    backend: Option<&crate::Backend>,
    function_loader: FunctionLoader<'_>,
) -> String {
    match context {
        DocblockContext::FunctionOrMethod => build_function(
            sym,
            form,
            content,
            position,
            decl_offset,
            use_map,
            file_namespace,
            local_classes,
            class_loader,
            resolved_class_loader,
            backend,
            function_loader,
        ),
        DocblockContext::ClassLike => build_class(sym, form, class_loader),
        DocblockContext::Property | DocblockContext::Constant => build_var(sym, form, class_loader),
        // Inline and Unknown are early-returned by the caller.
        DocblockContext::Inline | DocblockContext::Unknown => String::new(),
    }
}

/// Build the docblock for a function or method.
///
/// Only emits `@param` / `@return` tags when the native type needs
/// enrichment (missing, `array`, or class with `@template` params).
/// `@throws` tags are always emitted for uncaught exceptions.
/// Tags are grouped with blank `*` lines between groups.
/// Parameter names within the `@param` block are space-aligned.
#[allow(clippy::too_many_arguments)]
fn build_function(
    sym: &SymbolInfo,
    form: &mut DocForm<'_>,
    content: &str,
    position: Position,
    decl_offset: u32,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &SourceClassLoader<'_>,
    resolved_class_loader: &ResolvedClassLoader<'_>,
    backend: Option<&crate::Backend>,
    function_loader: FunctionLoader<'_>,
) -> String {
    let throws_ctx = ThrowsContext {
        class_loader,
        resolved_class_loader,
        function_loader,
        use_map,
        local_classes,
        backend,
    };
    let uncaught = throws_analysis::find_uncaught_throw_types_with_context(
        content,
        position,
        Some(&throws_ctx),
    );

    // Collect @param tags that need enrichment.
    // Each entry is (type, rendered width, name).
    let mut param_tags: Vec<(String, usize, String)> = Vec::new();
    for (type_hint, name) in &sym.params {
        if let Some((type_text, width)) =
            form.enrich(type_hint.as_ref(), class_loader, use_map, file_namespace)
        {
            param_tags.push((type_text, width, form.param_name(name)));
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
        // `list<string>` instead of `array<mixed>`).  Blanking leaves every
        // byte offset where it was, so `decl_offset` still applies.
        let offset = position_to_byte_offset(content, position).min(content.len());
        let content = throws_analysis::with_open_docblock_blanked(content, offset);
        let body_inferred = crate::code_actions::phpstan::fix_return_type::enrichment_return_type(
            FileCtx::new(&content, local_classes, resolved_class_loader, class_loader)
                .with_imports(Some(use_map))
                .with_backend(backend),
            decl_offset,
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
        match inferred {
            Some(t) => Some(
                crate::names::shorten_type_for_file(
                    &t,
                    use_map,
                    file_namespace,
                    resolved_class_loader,
                )
                .to_string(),
            ),
            None => form
                .enrich(
                    sym.return_type.as_ref(),
                    class_loader,
                    use_map,
                    file_namespace,
                )
                .map(|(type_text, _)| type_text),
        }
    };

    let has_throws = !uncaught.is_empty();

    let has_any_tag = !param_tags.is_empty() || has_throws || return_tag.is_some();

    let mut lines = vec!["/**".to_string()];

    if !has_any_tag {
        // No tags — emit a summary-only skeleton.
        lines.push(format!(" * {}", form.field("")));
    }

    // @param block with space-aligned names.
    let max_width = param_tags.iter().map(|(_, width, _)| *width).max();
    for (type_text, width, name) in &param_tags {
        let padding = " ".repeat(max_width.unwrap_or(0) - width);
        lines.push(format!(" * @param {type_text}{padding} {name}"));
    }

    // @throws block (blank separator from preceding group).
    if has_throws {
        if !param_tags.is_empty() {
            lines.push(" *".to_string());
        }
        for exc in &uncaught {
            let exc_str = exc.to_string();
            lines.push(format!(" * @throws {}", crate::util::short_name(&exc_str)));
        }
    }

    // @return tag (blank separator from preceding group).
    if let Some(ret) = return_tag {
        if !param_tags.is_empty() || has_throws {
            lines.push(" *".to_string());
        }
        lines.push(format!(" * @return {ret}"));
    }

    lines.push(" */".to_string());
    form.finish(&lines)
}

/// Build the docblock for a class, interface, trait, or enum.
///
/// Generates `@extends` / `@implements` tags, their type arguments open
/// for editing, when the parent or interface has `@template` parameters.
fn build_class(
    sym: &SymbolInfo,
    form: &mut DocForm<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let mut lines = vec!["/**".to_string()];

    for (tag, names) in [
        ("@extends", &sym.extends_names),
        ("@implements", &sym.implements_names),
    ] {
        for name in names {
            if let Some(cls) = class_loader(name)
                && !cls.template_params.is_empty()
            {
                let args: Vec<String> = cls
                    .template_params
                    .iter()
                    .map(|tp| form.field(tp))
                    .collect();
                lines.push(format!(" * {tag} {name}<{}>", args.join(", ")));
            }
        }
    }

    if lines.len() == 1 {
        // No template tags — emit a summary-only skeleton.
        lines.push(format!(" * {}", form.field("")));
    }

    lines.push(" */".to_string());
    form.finish(&lines)
}

/// Build the single-line `/** @var Type */` docblock for a property or
/// constant.
fn build_var(
    sym: &SymbolInfo,
    form: &mut DocForm<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let var_type = var_type(sym.type_hint.as_ref(), form, class_loader);
    form.finish(&[format!("/** @var {var_type} */")])
}

/// Attempt to infer the type of an inline variable assignment using the
/// hover type-resolution pipeline.
///
/// Given `$var = ['']`, this resolves to `list<string>` by delegating
/// to the same `resolve_variable_type` that powers hover.
pub(crate) fn infer_inline_variable_type(
    sym: &SymbolInfo,
    position: Position,
    file: FileCtx<'_>,
    function_loader: FunctionLoaderFn<'_>,
) -> Option<PhpType> {
    let content = file.content;
    let all_classes = file.all_classes;
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

    crate::type_engine::variable::resolution::resolve_variable_php_type(VarQuery {
        loaders: crate::type_engine::resolver::Loaders::with_function(function_loader),
        ..VarQuery::new(
            var_name,
            file.with_source(effective_content, all_classes),
            current_class,
            cursor_offset,
        )
    })
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

/// The `@var` type for a property or constant.
///
/// - Missing type → `mixed`, open for editing
/// - `array` → `array`, open for editing
/// - Class with templates → `ClassName<T1, ...>`, the arguments open for
///   editing
/// - Other → the type as written
fn var_type(
    type_hint: Option<&PhpType>,
    form: &mut DocForm<'_>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    match type_hint {
        None => form.field("mixed"),
        Some(th) if th.is_bare_array() => form.field("array"),
        Some(th) => {
            let shortened = th.shorten();
            // Callable types get a signature placeholder.
            if th.is_callable() {
                return format!("({})", form.field(&format!("{shortened}()")));
            }
            if let Some(name) = shortened.base_name()
                && let Some(cls) = class_loader(name)
                && !cls.template_params.is_empty()
            {
                let args: Vec<String> = cls
                    .template_params
                    .iter()
                    .map(|tp| form.field(tp))
                    .collect();
                return format!("{name}<{}>", args.join(", "));
            }
            shortened.to_string()
        }
    }
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
