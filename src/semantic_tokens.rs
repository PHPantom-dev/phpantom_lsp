//! Semantic Tokens (`textDocument/semanticTokens/full`).
//!
//! Provides type-aware syntax highlighting that goes beyond what a
//! TextMate grammar can achieve.  Classes, interfaces, enums,
//! properties, methods, parameters, and type hints all get distinct
//! token types.
//!
//! The implementation leverages the precomputed [`SymbolMap`] which
//! already contains classified spans (`ClassReference`, `FunctionCall`,
//! `MemberAccess`, `PropertyAccess`, `VariableReference`, etc.) with
//! byte offsets.  The main work is mapping these to LSP semantic token
//! types and computing the delta encoding.
//!
//! Language builtins (`self`, `static`, `parent`, `$this`) carry the
//! `defaultLibrary` modifier so that themes can distinguish them from
//! user-defined symbols.

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::config::SemanticTokensMode;
use crate::diagnostics::unknown_members::member_exists;
use crate::symbol_map::{ClassRefContext, SelfStaticParentKind, SymbolKind, SymbolMap, VarDefKind};
use crate::types::{ClassInfo, ClassLikeKind};

// ─── Token type indices ─────────────────────────────────────────────────────
//
// These constants define the position of each token type in the legend
// array.  The LSP protocol uses integer indices rather than names.
// All indices are referenced: some only by the legend array, others
// also by classification logic.

const TT_NAMESPACE: u32 = 0;
const TT_CLASS: u32 = 1;
const TT_INTERFACE: u32 = 2;
const TT_ENUM: u32 = 3;
const TT_TYPE: u32 = 4;
const TT_TYPE_PARAMETER: u32 = 5;
const TT_PARAMETER: u32 = 6;
const TT_VARIABLE: u32 = 7;
const TT_PROPERTY: u32 = 8;
const TT_FUNCTION: u32 = 9;
const TT_METHOD: u32 = 10;
const TT_DECORATOR: u32 = 11;
const TT_ENUM_MEMBER: u32 = 12;
const TT_KEYWORD: u32 = 13;
const TT_COMMENT: u32 = 14;

// ─── Token modifier bit positions ───────────────────────────────────────────

const TM_DECLARATION: u32 = 1 << 0;
const TM_STATIC: u32 = 1 << 1;
const TM_READONLY: u32 = 1 << 2;
const TM_DEPRECATED: u32 = 1 << 3;
const TM_ABSTRACT: u32 = 1 << 4;
const TM_DEFINITION: u32 = 1 << 5;
const TM_DEFAULT_LIBRARY: u32 = 1 << 6;

/// Build the semantic token legend that is advertised in `initialize`.
///
/// The order of types and modifiers here **must** match the index
/// constants above.
pub fn legend() -> SemanticTokensLegend {
    // Assert at compile time that every index constant has a matching
    // entry in the legend.  This also silences dead_code warnings for
    // constants that are only referenced by the legend (e.g. NAMESPACE).
    const _: () = {
        assert!(TT_NAMESPACE == 0);
        assert!(TT_CLASS == 1);
        assert!(TT_INTERFACE == 2);
        assert!(TT_ENUM == 3);
        assert!(TT_TYPE == 4);
        assert!(TT_TYPE_PARAMETER == 5);
        assert!(TT_PARAMETER == 6);
        assert!(TT_VARIABLE == 7);
        assert!(TT_PROPERTY == 8);
        assert!(TT_FUNCTION == 9);
        assert!(TT_METHOD == 10);
        assert!(TT_DECORATOR == 11);
        assert!(TT_ENUM_MEMBER == 12);
        assert!(TT_KEYWORD == 13);
        assert!(TT_COMMENT == 14);
    };

    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::NAMESPACE,      // 0
            SemanticTokenType::CLASS,          // 1
            SemanticTokenType::INTERFACE,      // 2
            SemanticTokenType::ENUM,           // 3
            SemanticTokenType::TYPE,           // 4
            SemanticTokenType::TYPE_PARAMETER, // 5
            SemanticTokenType::PARAMETER,      // 6
            SemanticTokenType::VARIABLE,       // 7
            SemanticTokenType::PROPERTY,       // 8
            SemanticTokenType::FUNCTION,       // 9
            SemanticTokenType::METHOD,         // 10
            SemanticTokenType::DECORATOR,      // 11
            SemanticTokenType::ENUM_MEMBER,    // 12
            SemanticTokenType::KEYWORD,        // 13
            SemanticTokenType::COMMENT,        // 14
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,     // bit 0
            SemanticTokenModifier::STATIC,          // bit 1
            SemanticTokenModifier::READONLY,        // bit 2
            SemanticTokenModifier::DEPRECATED,      // bit 3
            SemanticTokenModifier::ABSTRACT,        // bit 4
            SemanticTokenModifier::DEFINITION,      // bit 5
            SemanticTokenModifier::DEFAULT_LIBRARY, // bit 6
        ],
    }
}

/// A single absolute-positioned semantic token before delta encoding.
#[derive(Clone)]
struct AbsoluteToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

impl Backend {
    /// Handle a `textDocument/semanticTokens/full` request.
    ///
    /// Walks the file's precomputed [`SymbolMap`] and emits semantic
    /// tokens for every classified span.  For `ClassReference` spans
    /// the symbol is resolved to determine whether it is a class,
    /// interface, enum, or trait.
    pub fn handle_semantic_tokens_full(
        &self,
        uri: &str,
        content: &str,
    ) -> Option<SemanticTokensResult> {
        let symbol_map = self.symbol_maps.read().get(uri)?.clone();
        let ctx = self.file_context(uri);
        let mode = self.config().semantic_tokens.mode();

        if mode == SemanticTokensMode::Off {
            return Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: Vec::new(),
            }));
        }

        let vc_handle = self.blade_virtual_content.read();
        let effective_content = vc_handle.get(uri).map(|s| s.as_str()).unwrap_or(content);

        let mut tokens = self.collect_tokens(&symbol_map, effective_content, uri, &ctx, mode);

        // Sort by position (line, then character) to prepare for delta encoding.
        tokens.sort_by(|a, b| a.line.cmp(&b.line).then(a.start_char.cmp(&b.start_char)));

        // Translate tokens to Blade coordinates if necessary.
        if self.is_blade_file(uri) {
            let mut translated_tokens = Vec::with_capacity(tokens.len());
            for tok in tokens {
                let start_pos = Position {
                    line: tok.line,
                    character: tok.start_char,
                };
                let end_pos = Position {
                    line: tok.line,
                    character: tok.start_char + tok.length,
                };

                // A token inside the injected prologue highlights nothing the
                // template wrote.
                let (Some(start_translated), Some(end_translated)) = (
                    self.try_translate_php_to_blade(uri, start_pos),
                    self.try_translate_php_to_blade(uri, end_pos),
                ) else {
                    continue;
                };

                if start_translated.line != end_translated.line {
                    // Token spans across lines after translation? Skip it.
                    continue;
                }

                let new_length = end_translated
                    .character
                    .saturating_sub(start_translated.character);
                if new_length == 0 {
                    // Token became zero-width (e.g. was entirely inside a removed directive)
                    continue;
                }

                translated_tokens.push(AbsoluteToken {
                    line: start_translated.line,
                    start_char: start_translated.character,
                    length: new_length,
                    token_type: tok.token_type,
                    modifiers: tok.modifiers,
                });
            }
            tokens = translated_tokens;

            // Re-sort after translation as columns might have shifted significantly.
            tokens.sort_by(|a, b| a.line.cmp(&b.line).then(a.start_char.cmp(&b.start_char)));

            // Add Blade-native keyword tokens (directives, echo/comment delimiters)
            // directly in original Blade coordinates.  The `content` parameter
            // is the virtual PHP (swapped by `with_file_content`), so we must
            // read the original Blade source from `open_files`.
            if mode == SemanticTokensMode::Full
                && let Some(blade_content) = self.get_file_content(uri)
            {
                tokens.extend(Self::collect_blade_tokens(&blade_content));
            }

            // Re-sort to interleave Blade tokens with translated PHP tokens.
            tokens.sort_by(|a, b| a.line.cmp(&b.line).then(a.start_char.cmp(&b.start_char)));
        }

        // Deduplicate overlapping tokens at the same position (keep longer).
        tokens.dedup_by(|b, a| {
            if a.line == b.line && a.start_char == b.start_char {
                // Keep the longer token (swap b's fields into a if b is longer).
                if b.length > a.length {
                    a.length = b.length;
                    a.token_type = b.token_type;
                    a.modifiers = b.modifiers;
                }
                true
            } else {
                false
            }
        });

        let delta_tokens = encode_deltas(&tokens);

        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: delta_tokens,
        }))
    }

    /// Walk the symbol map and produce absolute-positioned tokens.
    fn collect_tokens(
        &self,
        symbol_map: &SymbolMap,
        content: &str,
        uri: &str,
        ctx: &crate::types::FileContext,
        mode: SemanticTokensMode,
    ) -> Vec<AbsoluteToken> {
        let Some(source) = symbol_map.source(content) else {
            return Vec::new();
        };

        let mut tokens = Vec::with_capacity(symbol_map.spans.len());

        // Precompute line starts once: converting each span's byte offset to a
        // line/column independently would rescan the file from the start every
        // time, which is O(n²) on large files (the demo file alone takes ~17s).
        let line_index = crate::text_position::LineIndex::new(content);

        let is_blade = self.is_blade_file(uri);

        for span in &symbol_map.spans {
            let length = span.end.saturating_sub(span.start);
            if length == 0 {
                continue;
            }

            let (token_type, modifiers) = match &span.kind {
                SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context,
                } => {
                    // Use-import names: only Blade files need these tokens
                    // (no PHP grammar is active there).  In regular PHP
                    // files the editor's own grammar (Tree-sitter/TextMate)
                    // already highlights the import prolog per name segment;
                    // a single token spanning the whole path (backslashes
                    // included) would visibly override that coloring.
                    if *context == ClassRefContext::Attribute {
                        if mode == SemanticTokensMode::Contextual {
                            continue;
                        }
                        (TT_DECORATOR, 0)
                    } else if *context == ClassRefContext::UseImport {
                        if !is_blade {
                            continue;
                        }
                        (TT_TYPE, 0)
                    } else if self.is_template_param(name, span.start, symbol_map) {
                        (TT_TYPE_PARAMETER, 0)
                    } else {
                        let mods = self.resolve_class_modifiers(name, *is_fqn, ctx, span.start);
                        if mode == SemanticTokensMode::Contextual && mods == 0 {
                            continue;
                        }
                        let tt = self.resolve_class_token_type(name, *is_fqn, ctx, span.start);
                        (tt, mods)
                    }
                }

                SymbolKind::ClassDeclaration { name } => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    let tt = self.resolve_declaration_token_type(name, uri, ctx);
                    let mut mods = TM_DECLARATION;
                    mods |= self.resolve_class_declaration_modifiers(name, uri, ctx);
                    (tt, mods)
                }

                SymbolKind::MemberAccess {
                    member_name,
                    is_static,
                    is_method_call,
                    subject_text,
                    docblock_ref,
                    is_array_callable: _,
                    is_nullsafe: _,
                } => {
                    let static_property_syntax = *is_static
                        && content
                            .get(span.start as usize..span.end as usize)
                            .is_some_and(|s| s.starts_with('$'));

                    // Verify the member exists on the resolved subject class
                    // and pick up deprecated/static modifiers from it.
                    match self.resolve_member_semantics(
                        subject_text.as_str(source),
                        member_name,
                        *is_static,
                        *is_method_call,
                        docblock_ref.is_reference(),
                        static_property_syntax,
                        span.start,
                        ctx,
                    ) {
                        Some((tt, mods)) => {
                            if mode == SemanticTokensMode::Contextual
                                && tt == TT_ENUM_MEMBER
                                && mods & TM_DEPRECATED == 0
                            {
                                continue;
                            }
                            if mode == SemanticTokensMode::Contextual && mods == 0 {
                                continue;
                            }
                            (tt, mods)
                        }
                        // The subject resolved to a known class and the
                        // member is verifiably absent — skip the token so
                        // the text keeps its default coloring (e.g. a plain
                        // string for `[Foo::class, 'mispelledMethod']`).
                        None => continue,
                    }
                }

                SymbolKind::MemberDeclaration { name, is_static } => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    let tt = self.classify_member_declaration(name, span.start, uri, ctx);
                    let mut mods = TM_DECLARATION;
                    if *is_static {
                        mods |= TM_STATIC;
                    }
                    (tt, mods)
                }

                SymbolKind::Variable { name } | SymbolKind::CompactVariable { name } => {
                    // Check if this variable is a parameter.
                    let (tt, mut mods) =
                        self.classify_variable(name, span.start, symbol_map, uri, ctx);
                    // Mark definitions.
                    if symbol_map.is_at_var_definition(name, span.start) {
                        mods |= TM_DEFINITION;
                    }
                    if mode == SemanticTokensMode::Contextual && tt == TT_VARIABLE {
                        continue;
                    }
                    (tt, mods)
                }

                SymbolKind::FunctionCall { is_definition, .. } => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    let mods = if *is_definition { TM_DECLARATION } else { 0 };
                    (TT_FUNCTION, mods)
                }

                SymbolKind::SelfStaticParent(ssp_kind) => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    match ssp_kind {
                        SelfStaticParentKind::This => {
                            (TT_VARIABLE, TM_READONLY | TM_DEFAULT_LIBRARY)
                        }
                        SelfStaticParentKind::Parent => {
                            let tt = self.resolve_self_static_parent_token_type(
                                ssp_kind, uri, ctx, span.start,
                            );
                            (tt, TM_DEFAULT_LIBRARY)
                        }
                        SelfStaticParentKind::Self_ | SelfStaticParentKind::Static => {
                            let tt = self.resolve_self_static_parent_token_type(
                                ssp_kind, uri, ctx, span.start,
                            );
                            (tt, TM_DEFAULT_LIBRARY)
                        }
                    }
                }

                SymbolKind::NamespaceDeclaration { .. } => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    // Same reasoning as use imports: leave the namespace
                    // declaration to the editor's own grammar in regular
                    // PHP files; emit only where no PHP grammar runs.
                    if !is_blade {
                        continue;
                    }
                    (TT_NAMESPACE, TM_DECLARATION)
                }

                SymbolKind::ConstantReference { .. } => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    // Check if this is a PHP attribute name (starts after `#[`).
                    let is_attr = span.start >= 2
                        && content
                            .get((span.start as usize).saturating_sub(2)..span.start as usize)
                            .is_some_and(|s| s.ends_with('#') || s.ends_with("["));
                    if is_attr {
                        (TT_DECORATOR, 0)
                    } else {
                        // Constants get the ENUM_MEMBER token type (standard LSP
                        // convention for constant-like values, including class
                        // constants and enum cases).
                        (TT_ENUM_MEMBER, TM_READONLY)
                    }
                }

                SymbolKind::Keyword => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    (TT_KEYWORD, 0)
                }

                SymbolKind::CastType => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    (TT_TYPE, 0)
                }

                SymbolKind::Comment => {
                    if mode == SemanticTokensMode::Contextual {
                        continue;
                    }
                    (TT_COMMENT, 0)
                }

                SymbolKind::LaravelStringKey { .. }
                | SymbolKind::LaravelMacroString { .. }
                | SymbolKind::CommandOwnParam { .. } => {
                    continue;
                }
            };

            if let Some(abs) = offset_to_absolute(
                content,
                &line_index,
                span.start,
                length,
                token_type,
                modifiers,
            ) {
                tokens.push(abs);
            }
        }

        // Split comment tokens around any inner tokens (e.g. class refs
        // and @var keywords inside docblocks).  Without this, a single
        // comment token covering `/** @var \App\Foo $x */` would hide
        // the more specific inner tokens.
        if mode == SemanticTokensMode::Full {
            for span in crate::phpstan_ignore::phpstan_ignore_tag_spans(content) {
                let length = span.end.saturating_sub(span.start) as u32;
                if length == 0 {
                    continue;
                }
                let position = line_index.position(span.start);
                if !crate::completion::comment_position::is_inside_non_doc_comment(
                    content, position,
                ) && !crate::completion::comment_position::is_inside_docblock(content, position)
                {
                    continue;
                }
                if let Some(abs) = offset_to_absolute(
                    content,
                    &line_index,
                    span.start as u32,
                    length,
                    TT_KEYWORD,
                    0,
                ) {
                    tokens.push(abs);
                }
            }

            for span in crate::phpstan_ignore::phpstan_ignore_code_spans(content) {
                let length = span.end.saturating_sub(span.start) as u32;
                if length == 0 {
                    continue;
                }
                let position = line_index.position(span.start);
                if !crate::completion::comment_position::is_inside_non_doc_comment(
                    content, position,
                ) && !crate::completion::comment_position::is_inside_docblock(content, position)
                {
                    continue;
                }
                if let Some(abs) = offset_to_absolute(
                    content,
                    &line_index,
                    span.start as u32,
                    length,
                    TT_ENUM_MEMBER,
                    0,
                ) {
                    tokens.push(abs);
                }
            }
        }

        split_comments_around_inner(&mut tokens);

        tokens
    }

    /// Resolve a class reference name to the appropriate token type
    /// (class, interface, enum, or type).
    fn resolve_class_token_type(
        &self,
        name: &str,
        is_fqn: bool,
        ctx: &crate::types::FileContext,
        offset: u32,
    ) -> u32 {
        let fqn = if is_fqn {
            name.to_string()
        } else {
            ctx.resolve_name_at(name, offset)
        };

        // First check in-file classes (fast path).
        for class in &ctx.classes {
            let class_fqn = match &class.file_namespace {
                Some(ns) => format!("{}\\{}", ns, class.name),
                None => class.name.to_string(),
            };
            if class_fqn == fqn || class.name == fqn {
                return kind_to_token_type(class.kind);
            }
        }

        // Try resolving from the global class index / stubs.
        if let Some(class_info) = self.find_or_load_class(&fqn) {
            return kind_to_token_type(class_info.kind);
        }

        // Fall back to CLASS for unresolved references.
        TT_CLASS
    }

    /// Resolve modifiers for a class reference (e.g. deprecated).
    fn resolve_class_modifiers(
        &self,
        name: &str,
        is_fqn: bool,
        ctx: &crate::types::FileContext,
        offset: u32,
    ) -> u32 {
        let fqn = if is_fqn {
            name.to_string()
        } else {
            ctx.resolve_name_at(name, offset)
        };

        // Check in-file classes.
        for class in &ctx.classes {
            let class_fqn = match &class.file_namespace {
                Some(ns) => format!("{}\\{}", ns, class.name),
                None => class.name.to_string(),
            };
            if class_fqn == fqn || class.name == fqn {
                if class.deprecation_message.is_some() {
                    return TM_DEPRECATED;
                }
                return 0;
            }
        }

        if let Some(class_info) = self.find_or_load_class(&fqn)
            && class_info.deprecation_message.is_some()
        {
            return TM_DEPRECATED;
        }

        0
    }

    /// Resolve the token type for a class declaration by looking up
    /// the class in the file's AST.
    fn resolve_declaration_token_type(
        &self,
        name: &str,
        _uri: &str,
        ctx: &crate::types::FileContext,
    ) -> u32 {
        for class in &ctx.classes {
            if class.name == name {
                return kind_to_token_type(class.kind);
            }
        }
        TT_CLASS
    }

    /// Resolve modifiers for a class declaration (deprecated, abstract).
    fn resolve_class_declaration_modifiers(
        &self,
        name: &str,
        _uri: &str,
        ctx: &crate::types::FileContext,
    ) -> u32 {
        let mut mods = 0u32;
        for class in &ctx.classes {
            if class.name == name {
                if class.deprecation_message.is_some() {
                    mods |= TM_DEPRECATED;
                }
                if class.is_abstract {
                    mods |= TM_ABSTRACT;
                }
                break;
            }
        }
        mods
    }

    /// Resolve member-level token type and modifiers by looking up the
    /// member in the subject's resolved class, and verify that the member
    /// exists at all.
    ///
    /// Returns:
    /// - `Some((token_type, mods))` — the member was found, or the subject
    ///   could not be cheaply resolved to a single class (variables, call
    ///   chains, unknown classes) and the token is emitted unmodified.
    /// - `None` — the subject resolved to a known class and the member is
    ///   verifiably absent (including magic-method fallbacks).  The caller
    ///   skips the token so the text keeps its default coloring.
    ///
    /// Only cheap, unambiguous subjects are resolved: bare class names
    /// (`Foo`, `App\Foo`, `\App\Foo`) and `$this`/`self`/`static`/`parent`
    /// via the enclosing class.  Anything else would require full
    /// expression resolution, which is too expensive to run for every
    /// member access on every semantic-tokens request.
    #[allow(clippy::too_many_arguments)]
    fn resolve_member_semantics(
        &self,
        subject_text: &str,
        member_name: &str,
        is_static: bool,
        is_method_call: bool,
        is_docblock_reference: bool,
        static_property_syntax: bool,
        offset: u32,
        ctx: &crate::types::FileContext,
    ) -> Option<(u32, u32)> {
        // The subject could not be verified — emit the token as-is.
        let unverified = Some((fallback_member_token_type(is_method_call), 0));

        // Docblock references (`@see Order::$channel_type`) use `::` for
        // every member kind, so the static/instance split below does not
        // apply.  Keep emitting unconditionally.
        if is_docblock_reference {
            return unverified;
        }

        let base: Arc<ClassInfo> = match subject_text {
            "$this" | "self" | "static" | "parent" => {
                // Innermost class whose byte range contains the access
                // (handles nested anonymous classes).
                let Some(enclosing) = ctx
                    .classes
                    .iter()
                    .filter(|c| offset >= c.start_offset && offset <= c.end_offset)
                    .max_by_key(|c| c.start_offset)
                else {
                    return unverified;
                };
                // Inside a trait, `$this`/`self`/`static` refer to the
                // (unknown) using class — members that live there cannot
                // be verified against the trait alone.
                if enclosing.kind == ClassLikeKind::Trait {
                    return unverified;
                }
                if subject_text == "parent" {
                    let Some(ref parent_name) = enclosing.parent_class else {
                        return unverified;
                    };
                    let fqn = ctx.resolve_name_at(parent_name, offset);
                    match self.find_or_load_class(&fqn) {
                        Some(c) => c,
                        None => return unverified,
                    }
                } else {
                    Arc::clone(enclosing)
                }
            }
            s if is_bare_class_name(s) => {
                let fqn = match s.strip_prefix('\\') {
                    Some(stripped) => stripped.to_string(),
                    None => ctx.resolve_name_at(s, offset),
                };
                match self.find_or_load_class(&fqn) {
                    Some(c) => c,
                    None => return unverified,
                }
            }
            // Variables and expression chains: full resolution is too
            // expensive here — emit unconditionally.
            _ => return unverified,
        };

        // stdClass is the universal object container — any member goes.
        if base.name == "stdClass" {
            return unverified;
        }

        let class_loader = |name: &str| self.find_or_load_class(name);
        let resolved = crate::virtual_members::resolve_class_fully_cached(
            &base,
            &class_loader,
            &self.resolved_class_cache,
        );

        if member_exists(&resolved, member_name, is_static, is_method_call) {
            let tt = member_access_token_type(
                &resolved,
                member_name,
                is_static,
                is_method_call,
                static_property_syntax,
            );
            let mods = member_extra_modifiers(
                &resolved,
                member_name,
                is_static,
                is_method_call,
                static_property_syntax,
            );
            return Some((tt, mods));
        }

        // Magic catch-alls: the member is dispatchable at runtime even
        // though it has no declaration — keep the token.
        if is_method_call {
            let magic = if is_static { "__callStatic" } else { "__call" };
            if resolved
                .methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(magic))
            {
                return unverified;
            }
        } else if !is_static
            && resolved
                .methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case("__get"))
        {
            return unverified;
        }

        None
    }

    /// Classify a MemberDeclaration as method, property, or constant.
    fn classify_member_declaration(
        &self,
        name: &str,
        offset: u32,
        _uri: &str,
        ctx: &crate::types::FileContext,
    ) -> u32 {
        // Find the enclosing class and look up the member.
        for class in &ctx.classes {
            if offset < class.start_offset || offset > class.end_offset {
                continue;
            }
            for method in &class.methods {
                if method.name == name {
                    return TT_METHOD;
                }
            }
            for prop in &class.properties {
                if prop.name == name {
                    return TT_PROPERTY;
                }
            }
            for constant in &class.constants {
                if constant.name == name {
                    return TT_ENUM_MEMBER;
                }
            }
        }

        // A `@property` / `@method` tag declares its member in the class
        // docblock, which sits *before* the class body the scan above
        // covers, so the name is classified from the tag that declared it
        // on the class the docblock is attached to.
        if let Some(class) = ctx
            .classes
            .iter()
            .filter(|c| offset < c.decl_start_offset)
            .min_by_key(|c| c.decl_start_offset)
        {
            if class.doc_properties().iter().any(|(prop, _)| prop == name) {
                return TT_PROPERTY;
            }
            if class
                .doc_methods()
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case(name))
            {
                return TT_METHOD;
            }
        }

        // Fall back to method if we can't determine.
        TT_METHOD
    }

    /// Classify a variable as parameter, property, or regular variable.
    fn classify_variable(
        &self,
        name: &str,
        offset: u32,
        symbol_map: &SymbolMap,
        _uri: &str,
        _ctx: &crate::types::FileContext,
    ) -> (u32, u32) {
        if let Some(kind) = symbol_map.var_def_kind_at(name, offset) {
            match kind {
                VarDefKind::Property => return (TT_PROPERTY, TM_DECLARATION),
                VarDefKind::Parameter => return (TT_PARAMETER, 0),
                _ => {}
            }
        }

        let scope = symbol_map.find_enclosing_scope(offset);
        for def in &symbol_map.var_defs {
            if def.name == name && def.scope_start == scope {
                match def.kind {
                    VarDefKind::Parameter => return (TT_PARAMETER, 0),
                    VarDefKind::Property => return (TT_PROPERTY, 0),
                    _ => {}
                }
            }
        }

        (TT_VARIABLE, 0)
    }

    /// Check whether a `ClassReference` name is actually a `@template`
    /// parameter that is in scope at the given offset.
    fn is_template_param(&self, name: &str, offset: u32, symbol_map: &SymbolMap) -> bool {
        symbol_map.find_template_def(name, offset).is_some()
    }

    /// Determine the token type for `self`, `static`, or `parent` by
    /// resolving to the enclosing class.
    fn resolve_self_static_parent_token_type(
        &self,
        ssp_kind: &crate::symbol_map::SelfStaticParentKind,
        _uri: &str,
        ctx: &crate::types::FileContext,
        offset: u32,
    ) -> u32 {
        if *ssp_kind == crate::symbol_map::SelfStaticParentKind::Parent {
            // Try to resolve the parent class kind.
            if let Some(class) = ctx.classes.first()
                && let Some(ref parent_name) = class.parent_class
            {
                let fqn = ctx.resolve_name_at(parent_name, offset);
                if let Some(parent_info) = self.find_or_load_class(&fqn) {
                    return kind_to_token_type(parent_info.kind);
                }
            }
        }
        TT_TYPE
    }

    /// Scan Blade source for directives, echo delimiters, and comments and
    /// emit semantic tokens in original Blade coordinates.
    ///
    /// Token type assignments:
    /// - Blade directives (`@if`, `@foreach`, etc.) → `keyword`
    /// - Echo delimiters (`{{ }}`, `{!! !!}`) → `keyword`
    /// - Comment blocks (`{{-- ... --}}`) → `comment` (entire span)
    ///
    /// The regions Blade itself does not compile (`{{-- --}}` comments,
    /// `@verbatim` blocks, `@php` blocks) come from the same
    /// [`inert_regions`] scan the directive-balance check and the `@props`
    /// reader use, and a candidate `@` is read by the same
    /// [`directive_head`] rule the formatter applies, so a `@@if` escape or
    /// a `@{{ … }}` literal echo is coloured as the text it compiles to.
    ///
    /// [`inert_regions`]: crate::blade::signature::inert_regions
    /// [`directive_head`]: crate::blade::directives::directive_head
    fn collect_blade_tokens(content: &str) -> Vec<AbsoluteToken> {
        use crate::blade::directives::{DirectiveHead, directive_head, match_directive};
        use crate::blade::signature::{InertOpener, echo_delimiters, inert_regions, is_echo_start};

        let bytes = content.as_bytes();
        let lines = crate::text_position::LineIndex::new(content);
        let mut tokens = Vec::new();
        // A token cannot span lines, so a range covering several is one
        // token per line, each measured in UTF-16 units from its line start.
        let mut push = |token_type: u32, range: std::ops::Range<usize>| {
            let mut at = range.start;
            for segment in content[range.clone()].split_inclusive('\n') {
                let text = segment.trim_end_matches(['\n', '\r']);
                if !text.is_empty() {
                    let from = lines.position(at);
                    let to = lines.position(at + text.len());
                    tokens.push(AbsoluteToken {
                        line: from.line,
                        start_char: from.character,
                        length: to.character - from.character,
                        token_type,
                        modifiers: 0,
                    });
                }
                at += segment.len();
            }
        };
        let directive_at = |at: usize| match directive_head(content, bytes, at, bytes.len()) {
            DirectiveHead::Named { name, name_end, .. } => (
                match_directive(name).map(|d| at..at + 1 + d.len()),
                name_end,
            ),
            DirectiveHead::None(end)
            | DirectiveHead::Escaped(end)
            | DirectiveHead::LiteralEcho(end) => (None, end),
        };

        let regions = inert_regions(content, true);
        let mut regions = regions.iter().peekable();
        let mut i = 0;
        while i < bytes.len() {
            while regions.peek().is_some_and(|region| region.span.end <= i) {
                regions.next();
            }
            if let Some(region) = regions.peek()
                && region.span.start <= i
            {
                if region.span.start == i {
                    match region.opener {
                        InertOpener::Comment => push(TT_COMMENT, region.span.clone()),
                        // The directives fencing the region are Blade;
                        // nothing between them is.
                        InertOpener::Verbatim
                        | InertOpener::PhpBlock
                        | InertOpener::PhpStatement => {
                            if let (Some(range), _) = directive_at(region.span.start) {
                                push(TT_KEYWORD, range);
                            }
                            let closer = match region.opener {
                                InertOpener::Verbatim => "@endverbatim".len(),
                                InertOpener::PhpBlock => "@endphp".len(),
                                _ => 0,
                            };
                            if region.terminated && closer > 0 {
                                push(TT_KEYWORD, region.span.end - closer..region.span.end);
                            }
                        }
                    }
                }
                i = region.span.end;
                regions.next();
                continue;
            }

            i = match bytes[i] {
                b'{' if is_echo_start(bytes, i) => {
                    let (open, _) = echo_delimiters(bytes, i);
                    push(TT_KEYWORD, i..i + open.len());
                    i + open.len()
                }
                b'}' if bytes[i..].starts_with(b"}}") && (i == 0 || bytes[i - 1] != b'}') => {
                    let len = if bytes[i..].starts_with(b"}}}") { 3 } else { 2 };
                    push(TT_KEYWORD, i..i + len);
                    i + len
                }
                b'!' if bytes[i..].starts_with(b"!!}") => {
                    push(TT_KEYWORD, i..i + 3);
                    i + 3
                }
                b'@' => {
                    let (range, end) = directive_at(i);
                    if let Some(range) = range {
                        push(TT_KEYWORD, range);
                    }
                    end
                }
                _ => i + 1,
            };
        }

        tokens
    }
}

/// Check whether a member-access subject is a bare class name
/// (`Foo`, `App\Foo`, `\App\Foo`) rather than a variable or an
/// expression chain.
fn is_bare_class_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '\\' || c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '\\')
}

fn fallback_member_token_type(is_method_call: bool) -> u32 {
    if is_method_call {
        TT_METHOD
    } else {
        TT_PROPERTY
    }
}

fn member_access_token_type(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    static_property_syntax: bool,
) -> u32 {
    if is_method_call {
        return TT_METHOD;
    }

    if is_static && !static_property_syntax && class.constants.iter().any(|c| c.name == member_name)
    {
        return TT_ENUM_MEMBER;
    }

    TT_PROPERTY
}

/// Collect modifier bits (deprecated, static, readonly) for a member that
/// is known to exist on the fully-resolved class.
fn member_extra_modifiers(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
    static_property_syntax: bool,
) -> u32 {
    let mut mods = 0;
    if is_method_call {
        if let Some(m) = class
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(member_name))
        {
            if m.is_static {
                mods |= TM_STATIC;
            }
            if m.deprecation_message.is_some() {
                mods |= TM_DEPRECATED;
            }
        }
    } else if is_static {
        mods |= TM_STATIC;
        // Constants first (most common in `Class::NAME` usage), then
        // static properties (`Class::$prop`).
        if !static_property_syntax
            && let Some(c) = class.constants.iter().find(|c| c.name == member_name)
        {
            mods |= TM_READONLY;
            if c.deprecation_message.is_some() {
                mods |= TM_DEPRECATED;
            }
        } else if let Some(p) = class.properties.iter().find(|p| {
            p.is_static && (p.name == member_name || format!("${}", p.name) == member_name)
        }) && p.deprecation_message.is_some()
        {
            mods |= TM_DEPRECATED;
        }
    } else if let Some(p) = class.properties.iter().find(|p| p.name == member_name) {
        if p.is_static {
            mods |= TM_STATIC;
        }
        if p.deprecation_message.is_some() {
            mods |= TM_DEPRECATED;
        }
    }
    mods
}

/// Map a [`ClassLikeKind`] to a semantic token type index.
fn kind_to_token_type(kind: ClassLikeKind) -> u32 {
    match kind {
        ClassLikeKind::Class => TT_CLASS,
        ClassLikeKind::Interface => TT_INTERFACE,
        ClassLikeKind::Trait => TT_TYPE,
        ClassLikeKind::Enum => TT_ENUM,
    }
}

/// Convert a byte offset and byte length to an absolute line/character
/// position and build an [`AbsoluteToken`].
///
/// `length` is a **byte** count (as stored in [`SymbolSpan`]).  This
/// function converts it to a UTF-16 code-unit count as required by the
/// LSP semantic token protocol.
///
/// Returns `None` if the offset is beyond the content length.
fn offset_to_absolute(
    content: &str,
    line_index: &crate::text_position::LineIndex,
    start_offset: u32,
    byte_length: u32,
    token_type: u32,
    modifiers: u32,
) -> Option<AbsoluteToken> {
    let start = start_offset as usize;
    let end = start + byte_length as usize;
    let text = content.get(start..end)?;
    let utf16_len: u32 = text.chars().map(|c| c.len_utf16() as u32).sum();
    if utf16_len == 0 {
        return None;
    }
    let pos = line_index.position(start);
    Some(AbsoluteToken {
        line: pos.line,
        start_char: pos.character,
        length: utf16_len,
        token_type,
        modifiers,
    })
}

/// Convert a list of absolute-positioned tokens into LSP delta-encoded
/// [`SemanticToken`] values.
fn encode_deltas(tokens: &[AbsoluteToken]) -> Vec<SemanticToken> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for tok in tokens {
        let delta_line = tok.line.saturating_sub(prev_line);
        let delta_start = if delta_line == 0 {
            tok.start_char.saturating_sub(prev_start)
        } else {
            tok.start_char
        };

        result.push(SemanticToken {
            delta_line,
            delta_start,
            length: tok.length,
            token_type: tok.token_type,
            token_modifiers_bitset: tok.modifiers,
        });

        prev_line = tok.line;
        prev_start = tok.start_char;
    }

    result
}

/// Split comment tokens around any non-comment tokens that fall inside them.
///
/// Docblock comments emit inner `Keyword` and `ClassReference` spans for
/// PHPDoc tags and type references (e.g. `@var \App\Foo`).  Because comment
/// spans are already split to one-per-line by the extraction layer, all inner
/// tokens are guaranteed to be on the same line as their enclosing comment
/// fragment.  The LSP protocol does not support overlapping tokens, so we
/// split each comment fragment around any inner tokens it contains.
fn split_comments_around_inner(tokens: &mut Vec<AbsoluteToken>) {
    // Sort by (line, start_char) so we can detect containment.
    tokens.sort_by(|a, b| a.line.cmp(&b.line).then(a.start_char.cmp(&b.start_char)));

    let mut new_tokens: Vec<AbsoluteToken> = Vec::with_capacity(tokens.len());

    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];

        // Skip non-comment tokens — they don't need splitting.
        if tok.token_type != TT_COMMENT {
            new_tokens.push(tokens[i].clone());
            i += 1;
            continue;
        }

        let comment_line = tok.line;
        let comment_start = tok.start_char;
        let comment_end = tok.start_char + tok.length;

        // Collect all non-comment tokens on the same line that fall within
        // this comment's range.
        let mut inner: Vec<&AbsoluteToken> = Vec::new();
        let mut j = i + 1;
        while j < tokens.len() && tokens[j].line == comment_line {
            let t = &tokens[j];
            if t.start_char >= comment_start
                && t.start_char + t.length <= comment_end
                && t.token_type != TT_COMMENT
            {
                inner.push(t);
            }
            if t.start_char >= comment_end {
                break;
            }
            j += 1;
        }

        if inner.is_empty() {
            // No inner tokens — keep the comment as-is.
            new_tokens.push(tokens[i].clone());
            i += 1;
            continue;
        }

        // Split the comment around the inner tokens.
        let mut cursor = comment_start;
        for inner_tok in &inner {
            // Comment fragment before this inner token.
            if inner_tok.start_char > cursor {
                new_tokens.push(AbsoluteToken {
                    line: comment_line,
                    start_char: cursor,
                    length: inner_tok.start_char - cursor,
                    token_type: TT_COMMENT,
                    modifiers: 0,
                });
            }
            // The inner token itself.
            new_tokens.push((*inner_tok).clone());
            cursor = inner_tok.start_char + inner_tok.length;
        }
        // Comment fragment after the last inner token.
        if cursor < comment_end {
            new_tokens.push(AbsoluteToken {
                line: comment_line,
                start_char: cursor,
                length: comment_end - cursor,
                token_type: TT_COMMENT,
                modifiers: 0,
            });
        }

        // Skip past the inner tokens we've already processed.
        // They'll be at positions i+1..j but we need to skip only
        // those that were part of `inner`.
        i += 1;
        while i < j {
            let t = &tokens[i];
            if t.line == comment_line
                && t.start_char >= comment_start
                && t.start_char + t.length <= comment_end
                && t.token_type != TT_COMMENT
            {
                // Already emitted as part of the split.
                i += 1;
            } else {
                break;
            }
        }
    }

    *tokens = new_tokens;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legend_has_correct_type_count() {
        let l = legend();
        // Ensure the legend has all the token types we reference.
        assert!(l.token_types.len() > TT_COMMENT as usize);
        assert_eq!(l.token_types.len(), 15);
        assert_eq!(l.token_modifiers.len(), 7);
    }

    #[test]
    fn delta_encoding_single_token() {
        let tokens = vec![AbsoluteToken {
            line: 3,
            start_char: 5,
            length: 10,
            token_type: TT_CLASS,
            modifiers: 0,
        }];
        let deltas = encode_deltas(&tokens);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].delta_line, 3);
        assert_eq!(deltas[0].delta_start, 5);
        assert_eq!(deltas[0].length, 10);
        assert_eq!(deltas[0].token_type, TT_CLASS);
    }

    #[test]
    fn delta_encoding_same_line() {
        let tokens = vec![
            AbsoluteToken {
                line: 1,
                start_char: 2,
                length: 3,
                token_type: TT_VARIABLE,
                modifiers: 0,
            },
            AbsoluteToken {
                line: 1,
                start_char: 10,
                length: 4,
                token_type: TT_METHOD,
                modifiers: 0,
            },
        ];
        let deltas = encode_deltas(&tokens);
        assert_eq!(deltas.len(), 2);
        // First token: absolute.
        assert_eq!(deltas[0].delta_line, 1);
        assert_eq!(deltas[0].delta_start, 2);
        // Second token: same line, relative start.
        assert_eq!(deltas[1].delta_line, 0);
        assert_eq!(deltas[1].delta_start, 8); // 10 - 2
    }

    #[test]
    fn delta_encoding_new_line() {
        let tokens = vec![
            AbsoluteToken {
                line: 1,
                start_char: 5,
                length: 3,
                token_type: TT_FUNCTION,
                modifiers: 0,
            },
            AbsoluteToken {
                line: 3,
                start_char: 2,
                length: 6,
                token_type: TT_CLASS,
                modifiers: TM_DECLARATION,
            },
        ];
        let deltas = encode_deltas(&tokens);
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[1].delta_line, 2); // 3 - 1
        assert_eq!(deltas[1].delta_start, 2); // absolute on new line
        assert_eq!(deltas[1].token_modifiers_bitset, TM_DECLARATION);
    }

    #[test]
    fn kind_to_token_type_mapping() {
        assert_eq!(kind_to_token_type(ClassLikeKind::Class), TT_CLASS);
        assert_eq!(kind_to_token_type(ClassLikeKind::Interface), TT_INTERFACE);
        assert_eq!(kind_to_token_type(ClassLikeKind::Enum), TT_ENUM);
        assert_eq!(kind_to_token_type(ClassLikeKind::Trait), TT_TYPE);
    }
}
