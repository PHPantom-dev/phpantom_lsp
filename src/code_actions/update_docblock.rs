//! Update Docblock to Match Signature code action.
//!
//! When a function or method signature changes (parameters added, removed,
//! reordered, or type hints updated), the docblock often falls out of sync.
//! This code action patches the `@param` and `@return` tags to match the
//! current signature while preserving descriptions and other tags.
//!
//! **Trigger:** Cursor is inside the docblock of a function/method whose
//! `@param` tags don't match the signature's parameters (by name, count,
//! or order), whose `@return` tag contradicts the return type hint, or
//! whose `@return` tag can be enriched with body-based type inference.
//!
//! **Code action kind:** `quickfix`.

use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
use mago_allocator::LocalArena;
use mago_docblock::document::TagKind;
use mago_span::HasSpan;
use mago_syntax::cst::class_like::member::ClassLikeMember;
use mago_syntax::cst::*;
use tower_lsp::lsp_types::*;

use super::cursor_context::{CursorContext, MemberContext, find_cursor_context};
use crate::Backend;
use crate::atom::bytes_to_str;
use crate::code_actions::phpstan::fix_return_type::enrichment_return_type;
use crate::completion::phpdoc::generation::{enrichment_plain, enrichment_plain_typed};
use crate::completion::source::throws_analysis::{self, ThrowsContext};
use crate::docblock::is_compatible_refinement_typed;
use crate::docblock::parser::{DocblockInfo, parse_docblock_for_tags};
use crate::docblock::type_strings::split_type_token;
use crate::parser::extract_hint_type;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionLoader};
use crate::util::{offset_to_position, short_name};

// ── Data types ──────────────────────────────────────────────────────────────

/// A parameter extracted from the function/method signature.
#[derive(Debug, Clone)]
struct SigParam {
    /// Parameter name including `$` prefix.
    name: String,
    /// Native type hint as a structured type, if present.
    type_hint: Option<PhpType>,
    /// Whether the parameter is variadic (`...$args`).
    is_variadic: bool,
}

/// A `@param` tag parsed from an existing docblock.
#[derive(Debug, Clone)]
struct DocParam {
    /// The original type string from the tag, preserved for docblock output.
    type_str_raw: String,
    /// The parsed type, constructed once from `type_str_raw`.
    type_parsed: PhpType,
    /// Parameter name including `$` prefix (and optional `...` prefix for variadic).
    name: String,
    /// Description text after the `$name`.
    description: String,
}

/// A `@return` tag parsed from an existing docblock.
#[derive(Debug, Clone)]
struct DocReturn {
    /// The parsed type, constructed once from the raw tag string.
    type_parsed: PhpType,
    /// Description text after the type.
    description: String,
}

/// Information about the function/method under the cursor, including its
/// docblock position and parsed tags.
struct FunctionWithDocblock {
    /// Byte range of the docblock comment (from `/**` to `*/` inclusive).
    docblock_start: usize,
    docblock_end: usize,
    /// The raw docblock text.
    docblock_text: String,
    /// Parameters from the signature.
    sig_params: Vec<SigParam>,
    /// Return type from the signature (if any).
    sig_return: Option<PhpType>,
    /// `@param` tags from the docblock.
    doc_params: Vec<DocParam>,
    /// `@return` tag from the docblock (if any).
    doc_return: Option<DocReturn>,
    /// `@throws` exception type names from the docblock.
    doc_throws: Vec<String>,
    /// Indentation of the docblock lines (whitespace before ` * `).
    indent: String,
    /// LSP position of the docblock start (for throws analysis).
    docblock_position: Position,
}

/// Resolve a type name to its FQN, using the class loader as the
/// source of truth.
///
/// Resolution order:
/// 1. Try `resolve_to_fqn` (use-map → namespace prefix → bare name).
/// 2. If the result can be loaded by the class loader, use the loaded FQN.
/// 3. Otherwise, try loading the original name directly (handles root-
///    namespace classes like `RuntimeException` that have no `use` import).
/// 4. Fall back to the `resolve_to_fqn` result.
fn resolve_type_name_to_fqn(
    name: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> String {
    let resolved = crate::util::resolve_to_fqn(name, use_map, file_namespace);
    // If the class loader recognises the resolved name, use its canonical FQN.
    if let Some(cls) = class_loader(&resolved) {
        return cls.fqn().to_string();
    }
    // The resolved name wasn't found — try the original name directly.
    // This handles root-namespace classes (e.g. `RuntimeException`) when
    // the file has a namespace but no explicit `use` import.
    if let Some(cls) = class_loader(name) {
        return cls.fqn().to_string();
    }
    // Neither worked — return the best guess.
    resolved
}

impl Backend {
    /// Collect "Update docblock" code actions for the function/method
    /// under the cursor.
    pub(crate) fn collect_update_docblock_actions(
        &self,
        uri: &str,
        content: &str,
        params: &CodeActionParams,
        out: &mut Vec<CodeActionOrCommand>,
    ) {
        let doc_uri: Url = match uri.parse() {
            Ok(u) => u,
            Err(_) => return,
        };

        let cursor_offset = crate::util::position_to_offset(content, params.range.start);

        // Resolve the function/method (and its docblock) under the cursor.
        // The returned info is owned, so the borrowed AST does not escape.
        let info =
            crate::parser::with_parsed_program(content, "update_docblock", |program, content| {
                let ctx = find_cursor_context(&program.statements, cursor_offset);
                let trivia = program.trivia.as_slice();
                find_function_with_docblock_from_context(
                    &ctx,
                    &program.statements,
                    trivia,
                    content,
                    cursor_offset,
                )
            });
        let info = match info {
            Some(info) => info,
            None => return,
        };

        // Build a class loader and function loader for type enrichment.
        let ctx = self.file_context(uri);
        let class_loader = self.class_loader(&ctx);
        let function_loader = self.function_loader(&ctx);

        // Determine if anything needs updating.
        let needs_update = check_needs_update(
            &info,
            content,
            &ctx.classes,
            &class_loader,
            Some(&function_loader),
            &ctx.use_map,
            &ctx.namespace,
        );
        if !needs_update {
            return;
        }

        // Build the replacement docblock.
        let new_docblock = build_updated_docblock(
            &info,
            content,
            &ctx.classes,
            &class_loader,
            Some(&function_loader),
            &ctx.use_map,
            &ctx.namespace,
        );
        if new_docblock == info.docblock_text {
            return;
        }

        let start_pos = offset_to_position(content, info.docblock_start);
        let end_pos = offset_to_position(content, info.docblock_end);

        let mut changes = HashMap::new();
        changes.insert(
            doc_uri,
            vec![TextEdit {
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
                new_text: new_docblock,
            }],
        );

        out.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: "Update docblock to match signature".to_string(),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: None,
            edit: Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }),
            command: None,
            is_preferred: Some(true),
            disabled: None,
            data: None,
        }));
    }
}

// ── AST walk ────────────────────────────────────────────────────────────────

/// Use the shared `CursorContext` to find the function/method at the cursor
/// position, then check for an existing docblock.
fn find_function_with_docblock_from_context<'a>(
    ctx: &CursorContext<'a>,
    statements: &'a Sequence<'a, Statement<'a>>,
    trivia: &[Trivia<'a>],
    content: &str,
    cursor: u32,
) -> Option<FunctionWithDocblock> {
    match ctx {
        CursorContext::InClassLike {
            member,
            all_members,
            ..
        } => {
            if let MemberContext::Method(method, _in_body) = member
                && cursor_on_docblock(cursor, method, trivia, content)
            {
                return build_info_for_function_like(
                    method.span().start.offset,
                    &method.parameter_list,
                    method.return_type_hint.as_ref(),
                    trivia,
                    content,
                );
            }
            // The cursor may be inside the docblock trivia that precedes
            // a method.  Docblocks live outside the method's AST span, so
            // `find_cursor_context` reports `MemberContext::None`.  Scan
            // all members to find a method whose preceding docblock
            // contains the cursor.
            if matches!(member, MemberContext::None) {
                for m in all_members.iter() {
                    if let ClassLikeMember::Method(method) = m
                        && cursor_on_docblock(cursor, method, trivia, content)
                    {
                        return build_info_for_function_like(
                            method.span().start.offset,
                            &method.parameter_list,
                            method.return_type_hint.as_ref(),
                            trivia,
                            content,
                        );
                    }
                }
            }
            None
        }
        CursorContext::InFunction(func, _in_body) => {
            if cursor_on_docblock(cursor, func, trivia, content) {
                return build_info_for_function_like(
                    func.span().start.offset,
                    &func.parameter_list,
                    func.return_type_hint.as_ref(),
                    trivia,
                    content,
                );
            }
            None
        }
        CursorContext::None => {
            // The cursor may be inside a docblock that precedes a
            // top-level (or namespace-level) standalone function.
            // Docblocks are trivia, so `find_cursor_context` returns
            // `None` when the cursor sits before the function's AST
            // span.  Scan all top-level statements to find a match.
            find_standalone_function_by_docblock(statements, trivia, content, cursor)
        }
    }
}

/// Scan top-level (and namespace-level) statements for a standalone
/// function whose preceding docblock contains the cursor.
fn find_standalone_function_by_docblock<'a>(
    statements: &'a Sequence<'a, Statement<'a>>,
    trivia: &[Trivia<'a>],
    content: &str,
    cursor: u32,
) -> Option<FunctionWithDocblock> {
    for stmt in statements.iter() {
        match stmt {
            Statement::Function(func) if cursor_on_docblock(cursor, func, trivia, content) => {
                return build_info_for_function_like(
                    func.span().start.offset,
                    &func.parameter_list,
                    func.return_type_hint.as_ref(),
                    trivia,
                    content,
                );
            }
            Statement::Namespace(ns) => {
                for s in ns.statements().iter() {
                    if let Statement::Function(func) = s
                        && cursor_on_docblock(cursor, func, trivia, content)
                    {
                        return build_info_for_function_like(
                            func.span().start.offset,
                            &func.parameter_list,
                            func.return_type_hint.as_ref(),
                            trivia,
                            content,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Check whether the cursor is inside the docblock trivia that immediately
/// precedes the given AST node.
fn cursor_on_docblock(
    cursor: u32,
    node: &impl HasSpan,
    trivia: &[Trivia<'_>],
    content: &str,
) -> bool {
    let node_start = node.span().start.offset;
    // Check if the cursor is inside the docblock that belongs to this node.
    // Uses the canonical trivia-based locator from symbol_map::docblock.
    if let Some((_text, db_start)) =
        crate::symbol_map::docblock::get_docblock_text_with_offset(trivia, content, node)
        && cursor >= db_start
        && cursor < node_start
    {
        return true;
    }
    false
}

/// Build a `FunctionWithDocblock` from a function-like AST node.
fn build_info_for_function_like<'a>(
    node_start: u32,
    param_list: &function_like::parameter::FunctionLikeParameterList<'a>,
    return_type_hint: Option<&function_like::r#return::FunctionLikeReturnTypeHint<'a>>,
    trivia: &[Trivia<'a>],
    content: &str,
) -> Option<FunctionWithDocblock> {
    // Find the docblock trivia immediately before this node.
    let candidate_idx = trivia.partition_point(|t| t.span.start.offset < node_start);
    if candidate_idx == 0 {
        return None;
    }

    let content_bytes = content.as_bytes();
    let mut covered_from = node_start;

    let mut docblock_trivia = None;
    for i in (0..candidate_idx).rev() {
        let t = &trivia[i];
        let t_end = t.span.end.offset;

        let gap = content_bytes
            .get(t_end as usize..covered_from as usize)
            .unwrap_or(&[]);
        if !gap.iter().all(u8::is_ascii_whitespace) {
            break;
        }

        match t.kind {
            TriviaKind::DocBlockComment => {
                docblock_trivia = Some(t);
                break;
            }
            TriviaKind::WhiteSpace
            | TriviaKind::SingleLineComment
            | TriviaKind::MultiLineComment
            | TriviaKind::HashComment => {
                covered_from = t.span.start.offset;
            }
        }
    }

    let trivia_node = docblock_trivia?;
    let docblock_start = trivia_node.span.start.offset as usize;
    let docblock_end = trivia_node.span.end.offset as usize;
    let docblock_text = content.get(docblock_start..docblock_end)?.to_string();

    // Extract signature parameters.
    let sig_params: Vec<SigParam> = param_list
        .parameters
        .iter()
        .map(|p| {
            let name = bytes_to_str(p.variable.name).to_string();
            let type_hint = p.hint.as_ref().map(|h| extract_hint_type(h));
            let is_variadic = p.ellipsis.is_some();
            SigParam {
                name,
                type_hint,
                is_variadic,
            }
        })
        .collect();

    // Extract return type.
    let sig_return = return_type_hint.map(|rth| extract_hint_type(&rth.hint));

    // Parse existing docblock tags with a single parse pass.
    let docblock_info = parse_docblock_for_tags(&docblock_text);
    let doc_params = docblock_info
        .as_ref()
        .map(parse_doc_params_from_info)
        .unwrap_or_default();
    let doc_return = docblock_info.as_ref().and_then(parse_doc_return_from_info);
    let doc_throws = docblock_info
        .as_ref()
        .map(parse_doc_throws_from_info)
        .unwrap_or_default();

    // Detect indentation.
    let indent = detect_indent(content, docblock_start);

    // Compute LSP position for throws analysis.
    let docblock_position = offset_to_position(content, docblock_start);

    Some(FunctionWithDocblock {
        docblock_start,
        docblock_end,
        docblock_text,
        sig_params,
        sig_return,
        doc_params,
        doc_return,
        doc_throws,
        indent,
        docblock_position,
    })
}

// ── Docblock parsing ────────────────────────────────────────────────────────

/// Parse all `@param` tags from a pre-parsed [`DocblockInfo`].
fn parse_doc_params_from_info(info: &DocblockInfo) -> Vec<DocParam> {
    let mut results = Vec::new();

    for tag in info.tags_by_kind(TagKind::Param) {
        let rest = tag.description.trim();
        if rest.is_empty() {
            continue;
        }

        // When the first token starts with `$` (or `...$` for variadic),
        // there is no type — the token is the parameter name directly.
        let first_token = rest.split_whitespace().next().unwrap_or("");
        let is_name_first = first_token.starts_with('$') || first_token.starts_with("...$");

        let (type_str, name_token, after_params) = if is_name_first {
            ("", first_token, &rest[first_token.len()..])
        } else {
            // Extract type token.
            let (type_str, remainder) = split_type_token(rest);
            let remainder = remainder.trim_start();

            // Extract parameter name.
            let name_token = remainder.split_whitespace().next().unwrap_or("");
            let after_params = remainder.get(name_token.len()..).unwrap_or("");
            (type_str, name_token, after_params)
        };

        if name_token.is_empty() || (!name_token.contains('$')) {
            continue;
        }

        let name = name_token.to_string();

        // mago-docblock joins continuation lines with \n; collapse to spaces
        // for the description to match the old behaviour.
        let description = after_params
            .trim()
            .lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ");

        results.push(DocParam {
            type_parsed: PhpType::parse(type_str),
            type_str_raw: type_str.to_string(),
            name,
            description,
        });
    }

    results
}

/// Parse the `@return` tag from a pre-parsed [`DocblockInfo`].
fn parse_doc_return_from_info(info: &DocblockInfo) -> Option<DocReturn> {
    for tag in info.tags_by_kind(TagKind::Return) {
        let rest = tag.description.trim();
        if rest.is_empty() {
            continue;
        }

        // Skip conditional return types.
        if rest.starts_with('(') {
            continue;
        }

        let (type_str, remainder) = split_type_token(rest);
        let description = remainder.trim().to_string();

        return Some(DocReturn {
            type_parsed: PhpType::parse(type_str),
            description,
        });
    }

    None
}

/// Parse `@throws` tags from a pre-parsed [`DocblockInfo`], returning
/// the exception type names.
fn parse_doc_throws_from_info(info: &DocblockInfo) -> Vec<String> {
    let mut results = Vec::new();
    for tag in info.tags_by_kind(TagKind::Throws) {
        let rest = tag.description.trim();
        if let Some(type_name) = rest.split_whitespace().next()
            && !type_name.is_empty()
        {
            results.push(type_name.to_string());
        }
    }
    results
}

/// Detect the indentation prefix from the source at the docblock position.
fn detect_indent(content: &str, docblock_start: usize) -> String {
    // Walk backward from docblock_start to find the line start.
    let before = &content[..docblock_start];
    let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
    let prefix = &content[line_start..docblock_start];
    // The indent is just whitespace.
    prefix.chars().take_while(|c| c.is_whitespace()).collect()
}

// ── Diff and update logic ───────────────────────────────────────────────────

/// Check whether the docblock needs updating.
fn check_needs_update(
    info: &FunctionWithDocblock,
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> bool {
    // Build a map of existing doc param names.
    let doc_param_names: Vec<&str> = info
        .doc_params
        .iter()
        .map(|p| {
            let n = p.name.as_str();
            n.strip_prefix("...").unwrap_or(n)
        })
        .collect();

    let sig_param_names: Vec<String> = info.sig_params.iter().map(|p| p.name.clone()).collect();

    // When the docblock already has at least one @param tag the user has
    // opted-in to documenting parameters, so every signature param is
    // relevant.  When the docblock has *zero* @param tags we only consider
    // params that need enrichment (matching generate-docblock behaviour).
    let has_any_doc_params = !doc_param_names.is_empty();

    if has_any_doc_params {
        // Check for missing, extra, or reordered params.
        if doc_param_names.len() != sig_param_names.len() {
            return true;
        }
        for (doc_name, sig_name) in doc_param_names.iter().zip(sig_param_names.iter()) {
            if *doc_name != sig_name.as_str() {
                return true;
            }
        }
    } else {
        // No @param tags at all — only flag if a param needs enrichment.
        let needs_enrichment = info
            .sig_params
            .iter()
            .any(|sp| enrichment_plain(sp.type_hint.as_ref(), class_loader).is_some());
        if needs_enrichment {
            return true;
        }
    }

    // Check for type contradictions in @param tags.
    for sig_param in &info.sig_params {
        if let Some(native_type) = &sig_param.type_hint
            && let Some(doc_param) = info.doc_params.iter().find(|dp| {
                let n = dp.name.as_str();
                let n = n.strip_prefix("...").unwrap_or(n);
                n == sig_param.name
            })
            && is_type_contradiction(&doc_param.type_parsed, native_type)
        {
            return true;
        }
    }

    // Check whether any existing @param type needs enrichment (e.g. a bare
    // `Closure` that should become `(Closure(): mixed)`, or a class with templates).
    // Skip when the doc type is already more specific (contains `<` or `(`).
    for sig_param in &info.sig_params {
        if let Some(doc_param) = info.doc_params.iter().find(|dp| {
            let n = dp.name.as_str();
            let n = n.strip_prefix("...").unwrap_or(n);
            n == sig_param.name
        }) {
            // If the doc type already carries generic params or a callable
            // signature, it is already enriched — no update needed.
            if doc_param.type_parsed.has_type_structure() {
                continue;
            }
            if let Some(enriched) =
                enrichment_plain_typed(sig_param.type_hint.as_ref(), class_loader)
                && !enriched.equivalent(&doc_param.type_parsed)
            {
                return true;
            }
        }
    }

    // Check @return tag.
    if let Some(sig_ret) = &info.sig_return
        && let Some(doc_ret) = &info.doc_return
    {
        // Remove `@return void` if the signature also has `: void`.
        if sig_ret.is_void() && doc_ret.type_parsed.is_void() {
            return true;
        }
        if is_type_contradiction(&doc_ret.type_parsed, sig_ret) {
            return true;
        }
    }

    // Check if the @return tag needs body-based enrichment.
    if let Some(sig_ret) = &info.sig_return
        && !sig_ret.is_void()
    {
        let doc_already_rich = info
            .doc_return
            .as_ref()
            .is_some_and(|dr| dr.type_parsed.has_type_structure());
        if !doc_already_rich
            && let Some(enriched) = enrichment_return_type(
                content,
                info.docblock_position,
                local_classes,
                class_loader,
                function_loader,
            )
            && !enriched.is_void()
            && !enriched.is_mixed()
            && !enriched.equivalent(sig_ret)
        {
            let differs_from_doc = info
                .doc_return
                .as_ref()
                .is_none_or(|dr| !dr.type_parsed.equivalent(&enriched));
            if differs_from_doc {
                return true;
            }
        }
    }

    // Check for missing @throws tags.
    let uncaught = throws_analysis::find_uncaught_throw_types_with_context(
        content,
        info.docblock_position,
        Some(&ThrowsContext {
            class_loader,
            function_loader,
            use_map,
            file_namespace,
        }),
    );
    let existing_fqns: Vec<String> = info
        .doc_throws
        .iter()
        .map(|t| resolve_type_name_to_fqn(t, use_map, file_namespace, class_loader).to_lowercase())
        .collect();
    for exc in &uncaught {
        let exc_str = exc.to_string();
        let exc_fqn = resolve_type_name_to_fqn(&exc_str, use_map, file_namespace, class_loader)
            .to_lowercase();
        if !existing_fqns.contains(&exc_fqn) {
            return true;
        }
    }

    false
}

/// Check if a docblock type contradicts a native type hint.
///
/// A contradiction means the docblock type is NOT a refinement of the native
/// type. For example, docblock says `string` but native says `int` is a
/// contradiction. But docblock says `non-empty-string` while native says
/// `string` is a refinement (not a contradiction).
fn is_type_contradiction(doc_type: &PhpType, native_type: &PhpType) -> bool {
    // `PhpType::equivalent` handles `?T` ↔ `T|null`, order-independent
    // unions, and FQN shortening.  It does not do case-insensitive
    // comparison, but PHP class names in practice come from the same
    // source so casing matches.
    if doc_type.equivalent(native_type) {
        return false;
    }

    // Check whether the docblock type is a compatible refinement of the
    // native type (e.g. `class-string<Foo>` refines `string`,
    // `list<User>` refines `array`, `positive-int` refines `int`).
    // This uses the shared refinement checker that also guards
    // `resolve_effective_type`.
    let native_core = native_type
        .non_null_type()
        .unwrap_or_else(|| native_type.clone());
    let doc_core = doc_type.non_null_type().unwrap_or_else(|| doc_type.clone());
    if is_compatible_refinement_typed(&doc_core, &native_core) {
        return false;
    }

    // For single-member types, compare base names directly.  If they
    // differ and neither is a refinement of the other, it is a
    // contradiction.
    let doc_bases = doc_type.union_members();
    let native_bases = native_type.union_members();

    if doc_bases.len() == 1
        && native_bases.len() == 1
        && !doc_bases[0].equivalent(native_bases[0])
        && !is_compatible_refinement_typed(doc_bases[0], native_bases[0])
    {
        return true;
    }

    false
}

/// Build the updated docblock text.
fn build_updated_docblock(
    info: &FunctionWithDocblock,
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    function_loader: FunctionLoader<'_>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    let indent = &info.indent;

    // Parse the existing docblock into lines, categorizing each line.
    let mut lines = parse_docblock_lines(&info.docblock_text);

    // Remove existing @param lines.
    lines.retain(|l| !matches!(l, DocLine::Param(_)));

    // Clean up orphaned empty lines left after removing @param lines.
    // Remove Empty lines that directly follow Open (no summary text).
    while lines.len() >= 2
        && matches!(lines[0], DocLine::Open)
        && matches!(lines[1], DocLine::Empty)
        && lines.get(2).is_some_and(|l| !matches!(l, DocLine::Text(_)))
    {
        lines.remove(1);
    }

    // Remove @return if it's redundant (void) or contradicted.
    let should_remove_return = should_remove_return(info);
    let should_update_return = should_update_return(info);
    if should_remove_return {
        lines.retain(|l| !matches!(l, DocLine::Return(_)));
    }

    // Find where to insert new @param lines.
    // Prefer inserting before the first @return or @throws, or at the end
    // before the closing `*/`.
    let insert_pos = find_param_insert_position(&lines);

    // Build new @param entries: (type_str, name_with_prefix, description).
    let param_entries: Vec<(String, String, String)> = info
        .sig_params
        .iter()
        .filter_map(|sig| {
            // Try to preserve the existing description for this param.
            let existing = info.doc_params.iter().find(|dp| {
                let n = dp.name.as_str();
                let n = n.strip_prefix("...").unwrap_or(n);
                n == sig.name
            });

            let has_any_doc_params = !info.doc_params.is_empty();

            let type_str = if let Some(existing) = existing {
                // If the existing type is a refinement, keep it.
                if let Some(native) = &sig.type_hint {
                    let native_str = native.to_string();
                    if is_type_contradiction(&existing.type_parsed, native) {
                        // Type is contradicted — try enrichment first, fall
                        // back to the raw native hint.
                        {
                            enrichment_plain(sig.type_hint.as_ref(), class_loader)
                                .unwrap_or(native_str)
                        }
                    } else if existing.type_parsed.has_type_structure() {
                        // Doc already has generics / callable / shape — keep it.
                        existing.type_str_raw.clone()
                    } else {
                        // Check if enrichment would upgrade the type (e.g.
                        // bare `Closure` → `(Closure(): mixed)`).
                        if let Some(enriched) =
                            enrichment_plain_typed(sig.type_hint.as_ref(), class_loader)
                        {
                            if !enriched.equivalent(&existing.type_parsed) {
                                enrichment_plain(sig.type_hint.as_ref(), class_loader)
                                    .unwrap_or_else(|| existing.type_str_raw.clone())
                            } else {
                                existing.type_str_raw.clone()
                            }
                        } else {
                            existing.type_str_raw.clone()
                        }
                    }
                } else {
                    existing.type_str_raw.clone()
                }
            } else if has_any_doc_params {
                // The docblock already documents some params, so add this
                // missing one — use enrichment or fall back to raw hint / mixed.
                {
                    enrichment_plain(sig.type_hint.as_ref(), class_loader).unwrap_or_else(|| {
                        sig.type_hint
                            .as_ref()
                            .map(|t| t.to_string())
                            .unwrap_or_else(|| PhpType::mixed().to_string())
                    })
                }
            } else {
                // No @param tags at all — only add a tag when the native
                // type needs enrichment, matching generate-docblock behaviour.
                enrichment_plain(sig.type_hint.as_ref(), class_loader)?
            };

            let description = existing.map(|e| e.description.clone()).unwrap_or_default();

            let name_prefix = if sig.is_variadic { "..." } else { "" };
            let full_name = format!("{}{}", name_prefix, sig.name);

            Some((type_str, full_name, description))
        })
        .collect();

    // Compute max type width for column alignment.
    let max_type_len = param_entries
        .iter()
        .map(|(t, _, _)| t.len())
        .max()
        .unwrap_or(0);

    // Build aligned @param DocLines.
    let new_params: Vec<DocLine> = param_entries
        .iter()
        .map(|(type_str, name, description)| {
            let padding = " ".repeat(max_type_len - type_str.len());
            let line_text = if description.is_empty() {
                format!("@param {}{} {}", type_str, padding, name)
            } else {
                format!("@param {}{} {} {}", type_str, padding, name, description)
            };
            DocLine::Param(line_text)
        })
        .collect();

    // Insert new param lines.
    for (i, param_line) in new_params.into_iter().enumerate() {
        lines.insert(insert_pos + i, param_line);
    }

    // Add missing @throws tags.
    let uncaught = throws_analysis::find_uncaught_throw_types_with_context(
        content,
        info.docblock_position,
        Some(&ThrowsContext {
            class_loader,
            function_loader,
            use_map,
            file_namespace,
        }),
    );
    let existing_fqns: Vec<String> = info
        .doc_throws
        .iter()
        .map(|t| resolve_type_name_to_fqn(t, use_map, file_namespace, class_loader).to_lowercase())
        .collect();

    let mut new_throws: Vec<String> = Vec::new();
    for exc in &uncaught {
        let exc_str = exc.to_string();
        let exc_fqn = resolve_type_name_to_fqn(&exc_str, use_map, file_namespace, class_loader)
            .to_lowercase();
        if !existing_fqns.contains(&exc_fqn) {
            // Use the short name in the generated @throws tag for readability
            let display_name = short_name(&exc_str);
            new_throws.push(display_name.to_string());
        }
    }

    if !new_throws.is_empty() {
        // Find the position to insert @throws — after the last existing
        // @throws tag, or after @param block, or before @return.
        let throws_insert_pos = find_throws_insert_position(&lines);
        for (i, exc) in new_throws.iter().enumerate() {
            lines.insert(
                throws_insert_pos + i,
                DocLine::OtherTag(format!("@throws {}", exc)),
            );
        }
    }

    // Update @return type if needed.
    if should_update_return
        && let Some(sig_ret) = &info.sig_return
        && let Some(doc_ret) = &info.doc_return
    {
        let sig_ret_str = sig_ret.to_string();
        // Find and update the return line.
        for line in &mut lines {
            if let DocLine::Return(text) = line {
                let description = &doc_ret.description;
                if description.is_empty() {
                    *text = format!("@return {}", sig_ret_str);
                } else {
                    *text = format!("@return {} {}", sig_ret_str, description);
                }
                break;
            }
        }
    }

    // Body-based @return enrichment.
    if let Some(sig_ret) = &info.sig_return
        && !sig_ret.is_void()
    {
        let has_rich_return = info
            .doc_return
            .as_ref()
            .is_some_and(|dr| dr.type_parsed.has_type_structure());
        if !has_rich_return
            && let Some(enriched) = enrichment_return_type(
                content,
                info.docblock_position,
                local_classes,
                class_loader,
                function_loader,
            )
            && !enriched.is_void()
            && !enriched.is_mixed()
            && !enriched.equivalent(sig_ret)
        {
            let differs_from_doc = info
                .doc_return
                .as_ref()
                .is_none_or(|dr| !dr.type_parsed.equivalent(&enriched));
            if differs_from_doc {
                // Update existing @return line or insert a new one.
                let mut updated_existing = false;
                for line in &mut lines {
                    if let DocLine::Return(text) = line {
                        // Preserve any description text after the type.
                        let desc = info
                            .doc_return
                            .as_ref()
                            .map(|dr| dr.description.as_str())
                            .unwrap_or("");
                        if desc.is_empty() {
                            *text = format!("@return {}", enriched);
                        } else {
                            *text = format!("@return {} {}", enriched, desc);
                        }
                        updated_existing = true;
                        break;
                    }
                }
                if !updated_existing {
                    // Insert before the closing `*/`.
                    let close_pos = lines
                        .iter()
                        .position(|l| matches!(l, DocLine::Close))
                        .unwrap_or(lines.len());
                    lines.insert(close_pos, DocLine::Return(format!("@return {}", enriched)));
                }
            }
        }
    }

    // Rebuild the docblock text.
    rebuild_docblock(&lines, indent)
}

/// Categorized docblock line.
#[derive(Debug, Clone)]
enum DocLine {
    /// Opening `/**`.
    Open,
    /// Closing `*/`.
    Close,
    /// A summary or description line (not a tag).
    Text(String),
    /// A `@param` tag line.
    Param(String),
    /// A `@return` tag line.
    Return(String),
    /// Any other tag line (`@throws`, `@template`, `@deprecated`, etc.).
    OtherTag(String),
    /// An empty line (just ` * `).
    Empty,
}

/// Parse a docblock into categorized lines.
fn parse_docblock_lines(docblock: &str) -> Vec<DocLine> {
    let mut result = Vec::new();
    let lines: Vec<&str> = docblock.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if i == 0 && trimmed.starts_with("/**") {
            // Single-line docblock: `/** @return void */`
            if trimmed.ends_with("*/") && trimmed.len() > 5 {
                let inner = trimmed
                    .strip_prefix("/**")
                    .unwrap_or("")
                    .strip_suffix("*/")
                    .unwrap_or("")
                    .trim();
                result.push(DocLine::Open);
                if !inner.is_empty() {
                    categorize_tag_line(inner, &mut result);
                }
                result.push(DocLine::Close);
                continue;
            }
            result.push(DocLine::Open);
            // Check if there's content after `/**` on the same line.
            let after_open = trimmed.strip_prefix("/**").unwrap_or("").trim();
            if !after_open.is_empty() {
                categorize_tag_line(after_open, &mut result);
            }
            continue;
        }

        if trimmed == "*/" || trimmed.ends_with("*/") {
            // Check if there's content before `*/`.
            let before_close = trimmed.strip_suffix("*/").unwrap_or("").trim();
            let before_close = before_close
                .strip_prefix('*')
                .unwrap_or(before_close)
                .trim();
            if !before_close.is_empty() {
                categorize_tag_line(before_close, &mut result);
            }
            result.push(DocLine::Close);
            continue;
        }

        // Regular docblock line: ` * content`
        let content = trimmed.strip_prefix('*').unwrap_or(trimmed).trim();

        // Check if this is a continuation line (no `@` prefix, preceded by
        // a tag line). If so, merge it into the previous tag line.
        if !content.is_empty()
            && !content.starts_with('@')
            && !result.is_empty()
            && matches!(
                result.last(),
                Some(DocLine::Param(_)) | Some(DocLine::Return(_)) | Some(DocLine::OtherTag(_))
            )
        {
            match result.last_mut() {
                Some(DocLine::Param(text))
                | Some(DocLine::Return(text))
                | Some(DocLine::OtherTag(text)) => {
                    text.push(' ');
                    text.push_str(content);
                }
                _ => {}
            }
            continue;
        }

        if content.is_empty() {
            result.push(DocLine::Empty);
        } else {
            categorize_tag_line(content, &mut result);
        }
    }

    result
}

/// Categorize a single content line (without the `*` prefix).
fn categorize_tag_line(content: &str, result: &mut Vec<DocLine>) {
    if content.starts_with("@param") {
        result.push(DocLine::Param(content.to_string()));
    } else if content.starts_with("@return") {
        result.push(DocLine::Return(content.to_string()));
    } else if content.starts_with('@') {
        result.push(DocLine::OtherTag(content.to_string()));
    } else {
        result.push(DocLine::Text(content.to_string()));
    }
}

/// Find the position to insert new `@param` lines.
fn find_param_insert_position(lines: &[DocLine]) -> usize {
    // Insert before the first @return, @throws, or other tag that comes
    // after any text/summary.
    let mut last_text_or_empty = None;
    let mut first_return_or_throws = None;

    for (i, line) in lines.iter().enumerate() {
        match line {
            DocLine::Text(_) | DocLine::Empty => {
                last_text_or_empty = Some(i);
            }
            DocLine::Return(_) if first_return_or_throws.is_none() => {
                first_return_or_throws = Some(i);
            }
            DocLine::OtherTag(text)
                if (text.starts_with("@throws") || text.starts_with("@return"))
                    && first_return_or_throws.is_none() =>
            {
                first_return_or_throws = Some(i);
            }
            _ => {}
        }
    }

    // Prefer inserting before @return/@throws.
    if let Some(pos) = first_return_or_throws {
        return pos;
    }

    // Otherwise insert after the last text/empty line.
    if let Some(pos) = last_text_or_empty {
        return pos + 1;
    }

    // Fallback: insert before Close.
    for (i, line) in lines.iter().enumerate() {
        if matches!(line, DocLine::Close) {
            return i;
        }
    }

    lines.len()
}

/// Find the position to insert new `@throws` lines.
fn find_throws_insert_position(lines: &[DocLine]) -> usize {
    // Insert after the last existing @throws tag.
    let mut last_throws = None;
    let mut first_return = None;

    for (i, line) in lines.iter().enumerate() {
        match line {
            DocLine::OtherTag(text) if text.starts_with("@throws") => {
                last_throws = Some(i);
            }
            DocLine::Return(_) if first_return.is_none() => {
                first_return = Some(i);
            }
            _ => {}
        }
    }

    // After the last existing @throws.
    if let Some(pos) = last_throws {
        return pos + 1;
    }

    // Before @return (but after any blank separator preceding it).
    if let Some(pos) = first_return {
        // If the line before @return is Empty, insert before that too.
        if pos > 0 && matches!(lines.get(pos - 1), Some(DocLine::Empty)) {
            return pos - 1;
        }
        return pos;
    }

    // After the last @param.
    let mut last_param = None;
    for (i, line) in lines.iter().enumerate() {
        if matches!(line, DocLine::Param(_)) {
            last_param = Some(i);
        }
    }
    if let Some(pos) = last_param {
        return pos + 1;
    }

    // Fallback: before Close.
    for (i, line) in lines.iter().enumerate() {
        if matches!(line, DocLine::Close) {
            return i;
        }
    }

    lines.len()
}

/// Check if the `@return` tag should be removed.
fn should_remove_return(info: &FunctionWithDocblock) -> bool {
    if let Some(sig_ret) = &info.sig_return
        && let Some(doc_ret) = &info.doc_return
        && sig_ret.is_void()
        && doc_ret.type_parsed.is_void()
        && doc_ret.description.is_empty()
    {
        return true;
    }
    false
}

/// Check if the `@return` tag needs its type updated.
fn should_update_return(info: &FunctionWithDocblock) -> bool {
    if let Some(sig_ret) = &info.sig_return
        && let Some(doc_ret) = &info.doc_return
        && is_type_contradiction(&doc_ret.type_parsed, sig_ret)
    {
        return true;
    }
    false
}

/// Rebuild a docblock string from categorized lines.
fn rebuild_docblock(lines: &[DocLine], indent: &str) -> String {
    let mut result = String::new();
    let mut prev_was_param = false;
    let mut prev_was_text_or_empty = false;

    for (i, line) in lines.iter().enumerate() {
        match line {
            DocLine::Open => {
                result.push_str("/**");
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
            DocLine::Close => {
                result.push_str(indent);
                result.push_str(" */");
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
            DocLine::Text(text) => {
                // Add blank separator before text if preceded by tags.
                if prev_was_param {
                    result.push_str(indent);
                    result.push_str(" *\n");
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = true;
            }
            DocLine::Empty => {
                result.push_str(indent);
                result.push_str(" *\n");
                prev_was_param = false;
                prev_was_text_or_empty = true;
            }
            DocLine::Param(text) => {
                // Add blank separator before first @param if preceded by text.
                if !prev_was_param && prev_was_text_or_empty {
                    // Check if the previous line was already empty.
                    let prev_empty = i > 0 && matches!(lines.get(i - 1), Some(DocLine::Empty));
                    if !prev_empty {
                        result.push_str(indent);
                        result.push_str(" *\n");
                    }
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = true;
                prev_was_text_or_empty = false;
            }
            DocLine::Return(text) => {
                // Add blank separator before @return if preceded by @param.
                if prev_was_param {
                    result.push_str(indent);
                    result.push_str(" *\n");
                }
                // Add blank separator if preceded by text without a blank line.
                if prev_was_text_or_empty && !prev_was_param {
                    let prev_empty = i > 0 && matches!(lines.get(i - 1), Some(DocLine::Empty));
                    if !prev_empty {
                        result.push_str(indent);
                        result.push_str(" *\n");
                    }
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
            DocLine::OtherTag(text) => {
                if prev_was_param {
                    result.push_str(indent);
                    result.push_str(" *\n");
                }
                result.push_str(indent);
                result.push_str(" * ");
                result.push_str(text);
                result.push('\n');
                prev_was_param = false;
                prev_was_text_or_empty = false;
            }
        }
    }

    result
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse PHP and check if an update is needed at the given offset.
    fn find_info(php: &str, offset: u32) -> Option<FunctionWithDocblock> {
        let arena = LocalArena::new();
        let file_id = mago_database::file::FileId::new(b"input.php");
        let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
        let ctx = find_cursor_context(&program.statements, offset);
        find_function_with_docblock_from_context(
            &ctx,
            &program.statements,
            program.trivia.as_slice(),
            php,
            offset,
        )
    }

    /// Stub class loader that never resolves anything (for unit tests).
    fn no_class_loader() -> impl Fn(&str) -> Option<Arc<ClassInfo>> {
        |_| None
    }

    /// No function loader (for unit tests).
    fn no_function_loader() -> FunctionLoader<'static> {
        None
    }

    #[test]
    fn detects_missing_param() {
        let php = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @param string $a The first param
     */
    public function bar(string $a, int $b): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn detects_extra_param() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     */
    public function bar(string $a): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn detects_reordered_params() {
        let php = r#"<?php
class Foo {
    /**
     * @param int $b
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
        let pos = php.find("@param int").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn no_update_when_params_match() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     */
    public function bar(string $a, int $b): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(!check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn detects_type_contradiction_in_param() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(int $a): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn preserves_refinement_type() {
        let php = r#"<?php
class Foo {
    /**
     * @param non-empty-string $a
     */
    public function bar(string $a): void {}
}
"#;
        let pos = php.find("@param non-empty-string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(!check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn detects_void_return_redundancy() {
        let php = r#"<?php
class Foo {
    /**
     * @return void
     */
    public function bar(): void {}
}
"#;
        let pos = php.find("@return void").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn detects_return_type_contradiction() {
        let php = r#"<?php
class Foo {
    /**
     * @return string
     */
    public function bar(): int {}
}
"#;
        let pos = php.find("@return string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn no_action_without_docblock() {
        let php = r#"<?php
class Foo {
    public function bar(string $a): void {}
}
"#;
        // No docblock at all — cursor on signature should return None.
        let pos = php.find("function bar").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(info.is_none());
    }

    #[test]
    fn works_with_standalone_function() {
        let php = r#"<?php
/**
 * @param string $a
 * @param int $b
 */
function bar(string $a, int $b, bool $c): void {}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn preserves_descriptions() {
        let php = r#"<?php
class Foo {
    /**
     * Summary line.
     *
     * @param string $a The first param
     */
    public function bar(string $a, int $b): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        assert!(
            updated.contains("The first param"),
            "Should preserve description: {}",
            updated
        );
        assert!(
            updated.contains("$b"),
            "Should add missing param: {}",
            updated
        );
        assert!(
            updated.contains("Summary line"),
            "Should preserve summary: {}",
            updated
        );
    }

    #[test]
    fn removes_extra_param_and_adds_missing() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $old
     * @param int $b
     */
    public function bar(int $b, bool $c): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        assert!(
            !updated.contains("$old"),
            "Should remove old param: {}",
            updated
        );
        assert!(updated.contains("$b"), "Should keep $b: {}", updated);
        assert!(updated.contains("$c"), "Should add $c: {}", updated);
    }

    #[test]
    fn updates_contradicted_return_type() {
        let php = r#"<?php
class Foo {
    /**
     * @return string Some description
     */
    public function bar(): int {}
}
"#;
        let pos = php.find("@return string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        assert!(
            updated.contains("@return int Some description"),
            "Should update return type: {}",
            updated
        );
    }

    #[test]
    fn removes_void_return() {
        let php = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @return void
     */
    public function bar(): void {}
}
"#;
        let pos = php.find("@return void").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        assert!(
            !updated.contains("@return"),
            "Should remove @return void: {}",
            updated
        );
    }

    #[test]
    fn handles_variadic_param() {
        let php = r#"<?php
class Foo {
    /**
     * @param string ...$args
     */
    public function bar(string ...$args): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        // Variadic params should match — no update needed.
        assert!(!check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn preserves_generic_refinement() {
        let php = r#"<?php
class Foo {
    /**
     * @param array<int, string> $items
     */
    public function bar(array $items): void {}
}
"#;
        let pos = php.find("@param array").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        // array<int, string> refines array — no contradiction.
        assert!(!check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn preserves_other_tags() {
        let php = r#"<?php
class Foo {
    /**
     * Summary.
     *
     * @template T
     * @param string $a
     * @throws \RuntimeException
     */
    public function bar(string $a, int $b): void {}
}
"#;
        let pos = php.find("@template T").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        assert!(
            updated.contains("@template T"),
            "Should preserve @template: {}",
            updated
        );
        assert!(
            updated.contains("@throws"),
            "Should preserve @throws: {}",
            updated
        );
        assert!(
            updated.contains("$b"),
            "Should add missing param: {}",
            updated
        );
    }

    #[test]
    fn is_contradiction_basic() {
        assert!(is_type_contradiction(
            &PhpType::parse("string"),
            &PhpType::parse("int")
        ));
        assert!(!is_type_contradiction(
            &PhpType::parse("string"),
            &PhpType::parse("string")
        ));
        assert!(!is_type_contradiction(
            &PhpType::parse("non-empty-string"),
            &PhpType::parse("string")
        ));
        assert!(!is_type_contradiction(
            &PhpType::parse("array<int, string>"),
            &PhpType::parse("array")
        ));
    }

    #[test]
    fn is_contradiction_nullable() {
        // ?string and string|null are equivalent.
        assert!(!is_type_contradiction(
            &PhpType::parse("?string"),
            &PhpType::parse("?string")
        ));
        assert!(!is_type_contradiction(
            &PhpType::parse("string|null"),
            &PhpType::parse("?string")
        ));
    }

    #[test]
    fn works_in_namespace() {
        let php = r#"<?php
namespace App;
class Foo {
    /**
     * @param string $a
     */
    public function bar(int $a): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn aligns_param_columns() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b, array $items): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        // All $names should be aligned at the same column.
        assert!(
            updated.contains("@param string       $a"),
            "Should have string padded: {}",
            updated
        );
        assert!(
            updated.contains("@param int          $b"),
            "Should have int padded: {}",
            updated
        );
        assert!(
            updated.contains("@param array<mixed> $items"),
            "Should have array<mixed> padded: {}",
            updated
        );
    }

    #[test]
    fn no_spurious_blank_line_after_open() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     * @param int $b
     *
     * @return string
     */
    public function bar(string $a, int $b, bool $c): string {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        // Should NOT have a blank line between /** and the first @param.
        let lines: Vec<&str> = updated.lines().collect();
        assert_eq!(
            lines[0].trim(),
            "/**",
            "First line should be opening: {}",
            updated
        );
        assert!(
            lines[1].trim().starts_with("* @param"),
            "Second line should be @param, not blank: {}",
            updated
        );
    }

    #[test]
    fn enriches_callable_types() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, Closure $handler, callable $fallback): void {}
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        assert!(
            updated.contains("(Closure(): mixed)"),
            "Should enrich Closure: {}",
            updated
        );
        assert!(
            updated.contains("(callable(): mixed)"),
            "Should enrich callable: {}",
            updated
        );
    }

    #[test]
    fn adds_missing_throws() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     *
     * @return string
     */
    public function bar(string $a): string {
        throw new \RuntimeException('oops');
    }
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        let updated = build_updated_docblock(
            &info,
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        );
        assert!(
            updated.contains("@throws RuntimeException"),
            "Should add missing @throws: {}",
            updated
        );
    }

    #[test]
    fn does_not_duplicate_existing_throws() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     *
     * @throws RuntimeException
     *
     * @return string
     */
    public function bar(string $a): string {
        throw new \RuntimeException('oops');
    }
}
"#;
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(
            !check_needs_update(
                &info,
                php,
                &[],
                &cl,
                no_function_loader(),
                &HashMap::new(),
                &None
            ),
            "Should not need update when throws already documented"
        );
    }

    #[test]
    fn triggers_when_cursor_inside_docblock() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
        // Place the cursor on the @param line inside the docblock.
        let pos = php.find("@param string").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_some(),
            "Should find function info when cursor is inside the docblock"
        );
        let cl = no_class_loader();
        assert!(check_needs_update(
            &info.unwrap(),
            php,
            &[],
            &cl,
            no_function_loader(),
            &HashMap::new(),
            &None,
        ));
    }

    #[test]
    fn triggers_when_cursor_on_docblock_summary() {
        let php = r#"<?php
class Foo {
    /**
     * Does something.
     *
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
        // Place the cursor on the summary line.
        let pos = php.find("Does something").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_some(),
            "Should find function info when cursor is on docblock summary"
        );
    }

    #[test]
    fn triggers_when_cursor_on_opening_docblock() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {}
}
"#;
        // Place the cursor on the /** line.
        let pos = php.find("/**").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_some(),
            "Should find function info when cursor is on opening /**"
        );
    }

    // ── @param with no type ─────────────────────────────────────────

    /// Helper: parse a docblock string into params via `_from_info`.
    fn test_parse_params(docblock: &str) -> Vec<DocParam> {
        match parse_docblock_for_tags(docblock) {
            Some(info) => parse_doc_params_from_info(&info),
            None => Vec::new(),
        }
    }

    #[test]
    fn parse_param_no_type_recognised() {
        let docblock = r#"/**
     * @param $name The user name
     */"#;
        let params = test_parse_params(docblock);
        assert_eq!(params.len(), 1, "should parse one param: {:?}", params);
        assert_eq!(params[0].name, "$name");
        assert_eq!(params[0].type_str_raw, "");
        assert_eq!(params[0].description, "The user name");
    }

    #[test]
    fn parse_param_no_type_variadic() {
        let docblock = r#"/**
     * @param ...$args The arguments
     */"#;
        let params = test_parse_params(docblock);
        assert_eq!(params.len(), 1, "should parse one param: {:?}", params);
        assert_eq!(params[0].name, "...$args");
        assert_eq!(params[0].type_str_raw, "");
        assert_eq!(params[0].description, "The arguments");
    }

    #[test]
    fn parse_param_no_type_no_description() {
        let docblock = r#"/**
     * @param $name
     */"#;
        let params = test_parse_params(docblock);
        assert_eq!(params.len(), 1, "should parse one param: {:?}", params);
        assert_eq!(params[0].name, "$name");
        assert_eq!(params[0].type_str_raw, "");
    }

    #[test]
    fn parse_param_no_type_mixed_with_typed() {
        let docblock = r#"/**
     * @param string $a First
     * @param $b Second
     * @param int $c Third
     */"#;
        let params = test_parse_params(docblock);
        assert_eq!(params.len(), 3, "should parse three params: {:?}", params);
        assert_eq!(params[0].name, "$a");
        assert_eq!(params[0].type_str_raw, "string");
        assert_eq!(params[1].name, "$b");
        assert_eq!(params[1].type_str_raw, "");
        assert_eq!(params[1].description, "Second");
        assert_eq!(params[2].name, "$c");
        assert_eq!(params[2].type_str_raw, "int");
    }

    #[test]
    fn update_needed_when_untyped_param_matches_untyped_sig() {
        // Even when both the docblock and signature omit the type, the
        // update action should fire to add `mixed` as the explicit type.
        let php = r#"<?php
class Foo {
    /**
     * @param $name The user name
     */
    public function bar($name): void {}
}
"#;
        let pos = php.find("@param $name").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(
            check_needs_update(
                &info,
                php,
                &[],
                &cl,
                no_function_loader(),
                &HashMap::new(),
                &None
            ),
            "should need update to add `mixed` type to @param $name"
        );
        // The param must still be recognised (not duplicated).
        assert_eq!(info.doc_params.len(), 1);
        assert_eq!(info.doc_params[0].name, "$name");
        assert_eq!(info.doc_params[0].type_str_raw, "");
        assert_eq!(info.doc_params[0].description, "The user name");
    }

    #[test]
    fn detects_missing_param_when_existing_has_no_type() {
        let php = r#"<?php
class Foo {
    /**
     * @param $a First param
     */
    public function bar(string $a, int $b): void {}
}
"#;
        let pos = php.find("@param $a").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(
            check_needs_update(
                &info,
                php,
                &[],
                &cl,
                no_function_loader(),
                &HashMap::new(),
                &None
            ),
            "should need update because $b is missing"
        );
        assert_eq!(info.doc_params.len(), 1);
        assert_eq!(info.doc_params[0].name, "$a");
        assert_eq!(info.doc_params[0].description, "First param");
    }

    #[test]
    fn no_update_for_empty_docblock_with_fully_typed_params() {
        // When generate-docblock produces `/** */` (no @param tags) because
        // the native types are sufficient, update-docblock should NOT offer
        // to add redundant @param tags.
        let php = r#"<?php
class Foo {
    /**
     *
     */
    public function stepIntro(CustomerRequest $request): View {}
}
"#;
        let pos = php.find("/**").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(
            !check_needs_update(
                &info,
                php,
                &[],
                &cl,
                no_function_loader(),
                &HashMap::new(),
                &None
            ),
            "should not suggest adding @param for a fully-typed non-templated class param"
        );
    }

    #[test]
    fn no_update_for_empty_docblock_with_scalar_params() {
        let php = r#"<?php
class Foo {
    /**
     *
     */
    public function bar(string $a, int $b, bool $c): void {}
}
"#;
        let pos = php.find("/**").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(
            !check_needs_update(
                &info,
                php,
                &[],
                &cl,
                no_function_loader(),
                &HashMap::new(),
                &None
            ),
            "should not suggest adding @param for scalar-typed params"
        );
    }

    #[test]
    fn update_for_empty_docblock_with_untyped_param() {
        // When a param has no native type, enrichment produces `mixed`,
        // so the update should be offered.
        let php = r#"<?php
class Foo {
    /**
     *
     */
    public function bar($untyped): void {}
}
"#;
        let pos = php.find("/**").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(
            check_needs_update(
                &info,
                php,
                &[],
                &cl,
                no_function_loader(),
                &HashMap::new(),
                &None
            ),
            "should suggest adding @param for an untyped param"
        );
    }

    #[test]
    fn update_for_empty_docblock_with_array_param() {
        // `array` is enrichable (stays `array` but signals it needs a shape
        // or value-type annotation), so the update should be offered.
        let php = r#"<?php
class Foo {
    /**
     *
     */
    public function bar(array $items): void {}
}
"#;
        let pos = php.find("/**").unwrap() as u32;
        let info = find_info(php, pos).unwrap();
        let cl = no_class_loader();
        assert!(
            check_needs_update(
                &info,
                php,
                &[],
                &cl,
                no_function_loader(),
                &HashMap::new(),
                &None
            ),
            "should suggest adding @param for an array param"
        );
    }

    #[test]
    fn no_info_inside_method_body() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
        // Place cursor on `$x = 1;` inside the method body.
        let pos = php.find("$x = 1").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_none(),
            "should not offer update docblock inside method body"
        );
    }

    #[test]
    fn no_info_on_method_opening_brace() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
        // Place cursor on the opening brace of the method body.
        let pos = php.find("{\n        $x").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_none(),
            "should not offer update docblock on method body brace"
        );
    }

    #[test]
    fn no_info_on_method_name() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
        let pos = php.find("bar").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_none(),
            "should not offer update docblock when cursor is on method name"
        );
    }

    #[test]
    fn no_info_on_method_return_type() {
        let php = r#"<?php
class Foo {
    /**
     * @param string $a
     */
    public function bar(string $a, int $b): void {
        $x = 1;
    }
}
"#;
        let pos = php.find("void").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_none(),
            "should not offer update docblock when cursor is on return type hint"
        );
    }

    #[test]
    fn no_info_inside_standalone_function_body() {
        let php = r#"<?php
/**
 * @param string $a
 */
function foo(string $a, int $b): void {
    $x = 1;
}
"#;
        let pos = php.find("$x = 1").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_none(),
            "should not offer update docblock inside standalone function body"
        );
    }

    #[test]
    fn no_info_on_standalone_function_signature() {
        let php = r#"<?php
/**
 * @param string $a
 */
function foo(string $a, int $b): void {
    $x = 1;
}
"#;
        let pos = php.find("function foo").unwrap() as u32;
        let info = find_info(php, pos);
        assert!(
            info.is_none(),
            "should not offer update docblock when cursor is on standalone function signature"
        );
    }
}
