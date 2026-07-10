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
use crate::diagnostics::unknown_members::member_exists;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind, SymbolMap, VarDefKind};
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

        let vc_handle = self.blade_virtual_content.read();
        let effective_content = vc_handle.get(uri).map(|s| s.as_str()).unwrap_or(content);

        let mut tokens = self.collect_tokens(&symbol_map, effective_content, uri, &ctx);

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

                let start_translated = self.translate_php_to_blade(uri, start_pos);
                let end_translated = self.translate_php_to_blade(uri, end_pos);

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
            if let Some(blade_content) = self.get_file_content(uri) {
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
    ) -> Vec<AbsoluteToken> {
        let mut tokens = Vec::with_capacity(symbol_map.spans.len());

        // Precompute line starts once: converting each span's byte offset to a
        // line/column independently would rescan the file from the start every
        // time, which is O(n²) on large files (the demo file alone takes ~17s).
        let line_index = crate::util::LineIndex::new(content);

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
                    if *context == crate::symbol_map::ClassRefContext::UseImport {
                        if !is_blade {
                            continue;
                        }
                        (TT_TYPE, 0)
                    } else if self.is_template_param(name, span.start, symbol_map) {
                        (TT_TYPE_PARAMETER, 0)
                    } else {
                        let tt = self.resolve_class_token_type(name, *is_fqn, ctx, span.start);
                        let mods = self.resolve_class_modifiers(name, *is_fqn, ctx, span.start);
                        (tt, mods)
                    }
                }

                SymbolKind::ClassDeclaration { name } => {
                    let tt = self.resolve_declaration_token_type(name, uri, ctx);
                    let mut mods = TM_DECLARATION;
                    // Check if the declared class itself is deprecated or abstract.
                    mods |= self.resolve_class_declaration_modifiers(name, uri, ctx);
                    (tt, mods)
                }

                SymbolKind::MemberAccess {
                    member_name,
                    is_static,
                    is_method_call,
                    subject_text,
                    is_docblock_reference,
                } => {
                    let tt = if *is_method_call {
                        TT_METHOD
                    } else {
                        TT_PROPERTY
                    };
                    let base_mods = if *is_static { TM_STATIC } else { 0 };

                    // Verify the member exists on the resolved subject class
                    // and pick up deprecated/static modifiers from it.
                    match self.resolve_member_modifiers(
                        subject_text,
                        member_name,
                        *is_static,
                        *is_method_call,
                        *is_docblock_reference,
                        span.start,
                        ctx,
                    ) {
                        Some(mods) => (tt, base_mods | mods),
                        // The subject resolved to a known class and the
                        // member is verifiably absent — skip the token so
                        // the text keeps its default coloring (e.g. a plain
                        // string for `[Foo::class, 'mispelledMethod']`).
                        None => continue,
                    }
                }

                SymbolKind::MemberDeclaration { name, is_static } => {
                    // Determine if it's a method, property, or constant
                    // by checking the source text at the span.
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
                    (tt, mods)
                }

                SymbolKind::FunctionCall {
                    name: _,
                    is_definition,
                } => {
                    let mods = if *is_definition { TM_DECLARATION } else { 0 };
                    (TT_FUNCTION, mods)
                }

                SymbolKind::SelfStaticParent(ssp_kind) => match ssp_kind {
                    SelfStaticParentKind::This => (TT_VARIABLE, TM_READONLY | TM_DEFAULT_LIBRARY),
                    SelfStaticParentKind::Parent => {
                        let tt = self
                            .resolve_self_static_parent_token_type(ssp_kind, uri, ctx, span.start);
                        (tt, TM_DEFAULT_LIBRARY)
                    }
                    SelfStaticParentKind::Self_ | SelfStaticParentKind::Static => {
                        let tt = self
                            .resolve_self_static_parent_token_type(ssp_kind, uri, ctx, span.start);
                        (tt, TM_DEFAULT_LIBRARY)
                    }
                },

                SymbolKind::NamespaceDeclaration { .. } => {
                    // Same reasoning as use imports: leave the namespace
                    // declaration to the editor's own grammar in regular
                    // PHP files; emit only where no PHP grammar runs.
                    if !is_blade {
                        continue;
                    }
                    (TT_NAMESPACE, TM_DECLARATION)
                }

                SymbolKind::ConstantReference { name: _ } => {
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

                SymbolKind::Keyword => (TT_KEYWORD, 0),

                SymbolKind::CastType => (TT_TYPE, 0),

                SymbolKind::Comment => (TT_COMMENT, 0),

                SymbolKind::LaravelStringKey { .. } => continue,
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

    /// Resolve member-level modifiers (deprecated, readonly, static) by
    /// looking up the member in the subject's resolved class, and verify
    /// that the member exists at all.
    ///
    /// Returns:
    /// - `Some(mods)` — the member was found (`mods` carries its
    ///   deprecated/static/readonly bits), or the subject could not be
    ///   cheaply resolved to a single class (variables, call chains,
    ///   unknown classes) and the token is emitted unmodified.
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
    fn resolve_member_modifiers(
        &self,
        subject_text: &str,
        member_name: &str,
        is_static: bool,
        is_method_call: bool,
        is_docblock_reference: bool,
        offset: u32,
        ctx: &crate::types::FileContext,
    ) -> Option<u32> {
        /// The subject could not be verified — emit the token as-is.
        const UNVERIFIED: Option<u32> = Some(0);

        // Docblock references (`@see Order::$channel_type`) use `::` for
        // every member kind, so the static/instance split below does not
        // apply.  Keep emitting unconditionally.
        if is_docblock_reference {
            return UNVERIFIED;
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
                    return UNVERIFIED;
                };
                // Inside a trait, `$this`/`self`/`static` refer to the
                // (unknown) using class — members that live there cannot
                // be verified against the trait alone.
                if enclosing.kind == ClassLikeKind::Trait {
                    return UNVERIFIED;
                }
                if subject_text == "parent" {
                    let Some(ref parent_name) = enclosing.parent_class else {
                        return UNVERIFIED;
                    };
                    let fqn = ctx.resolve_name_at(parent_name, offset);
                    match self.find_or_load_class(&fqn) {
                        Some(c) => c,
                        None => return UNVERIFIED,
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
                    None => return UNVERIFIED,
                }
            }
            // Variables and expression chains: full resolution is too
            // expensive here — emit unconditionally.
            _ => return UNVERIFIED,
        };

        // stdClass is the universal object container — any member goes.
        if base.name == "stdClass" {
            return UNVERIFIED;
        }

        let class_loader = |name: &str| self.find_or_load_class(name);
        let resolved = crate::virtual_members::resolve_class_fully_cached(
            &base,
            &class_loader,
            &self.resolved_class_cache,
        );

        if member_exists(&resolved, member_name, is_static, is_method_call) {
            return Some(member_extra_modifiers(
                &resolved,
                member_name,
                is_static,
                is_method_call,
            ));
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
                return UNVERIFIED;
            }
        } else if !is_static
            && resolved
                .methods
                .iter()
                .any(|m| m.name.eq_ignore_ascii_case("__get"))
        {
            return UNVERIFIED;
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
            // Check methods.
            for method in &class.methods {
                if method.name == name {
                    return TT_METHOD;
                }
            }
            // Check properties.
            for prop in &class.properties {
                if prop.name == name {
                    return TT_PROPERTY;
                }
            }
            // Check constants / enum cases.
            for constant in &class.constants {
                if constant.name == name {
                    return TT_ENUM_MEMBER;
                }
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
        // Check if this is a property declaration.
        if let Some(kind) = symbol_map.var_def_kind_at(name, offset) {
            match kind {
                VarDefKind::Property => return (TT_PROPERTY, TM_DECLARATION),
                VarDefKind::Parameter => return (TT_PARAMETER, 0),
                _ => {}
            }
        }

        // Check if any VarDefSite marks this variable as a parameter
        // in the current scope.
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

    /// Scan Blade source for directives, echo delimiters, and comment
    /// delimiters and emit semantic tokens in original Blade coordinates.
    ///
    /// Token type assignments:
    /// - Blade directives (`@if`, `@foreach`, etc.) → `keyword`
    /// - Echo delimiters (`{{ }}`, `{!! !!}`) → `keyword`
    /// - Comment blocks (`{{-- ... --}}`) → `comment` (entire span)
    fn collect_blade_tokens(content: &str) -> Vec<AbsoluteToken> {
        let mut tokens = Vec::new();
        let mut in_comment = false;

        for (line_idx, line) in content.lines().enumerate() {
            let line_u32 = line_idx as u32;
            let chars: Vec<char> = line.chars().collect();
            let mut col = 0u32; // UTF-16 column
            let mut i = 0usize;

            // If we're inside a multi-line comment, mark the entire line
            // as comment until we find --}}.
            if in_comment {
                if let Some(close_pos) = find_substr(&chars, 0, &['-', '-', '}', '}']) {
                    // Comment ends on this line: mark from start to end of --}}
                    let end_col = utf16_col_at(&chars, close_pos + 4);
                    tokens.push(AbsoluteToken {
                        line: line_u32,
                        start_char: 0,
                        length: end_col,
                        token_type: TT_COMMENT,
                        modifiers: 0,
                    });
                    in_comment = false;
                    // Continue scanning the rest of the line after the comment.
                    i = close_pos + 4;
                    col = end_col;
                } else {
                    // Entire line is comment.
                    let line_len: u32 = chars.iter().map(|c| c.len_utf16() as u32).sum();
                    if line_len > 0 {
                        tokens.push(AbsoluteToken {
                            line: line_u32,
                            start_char: 0,
                            length: line_len,
                            token_type: TT_COMMENT,
                            modifiers: 0,
                        });
                    }
                    continue;
                }
            }

            while i < chars.len() {
                let remaining = &chars[i..];

                // {{-- comment start
                if remaining.starts_with(&['{', '{', '-', '-']) {
                    if let Some(close_pos) = find_substr(&chars, i + 4, &['-', '-', '}', '}']) {
                        // Single-line comment: mark the entire {{-- ... --}} span.
                        let end_col = utf16_col_at(&chars, close_pos + 4);
                        tokens.push(AbsoluteToken {
                            line: line_u32,
                            start_char: col,
                            length: end_col - col,
                            token_type: TT_COMMENT,
                            modifiers: 0,
                        });
                        i = close_pos + 4;
                        col = end_col;
                        continue;
                    } else {
                        // Multi-line comment starts here.
                        let line_len: u32 = chars.iter().map(|c| c.len_utf16() as u32).sum();
                        tokens.push(AbsoluteToken {
                            line: line_u32,
                            start_char: col,
                            length: line_len - col,
                            token_type: TT_COMMENT,
                            modifiers: 0,
                        });
                        in_comment = true;
                        break;
                    }
                }

                // {!! raw echo !!}
                if remaining.starts_with(&['{', '!', '!']) {
                    tokens.push(AbsoluteToken {
                        line: line_u32,
                        start_char: col,
                        length: 3,
                        token_type: TT_KEYWORD,
                        modifiers: 0,
                    });
                    i += 3;
                    col += 3;
                    continue;
                }

                // !!} closing raw echo
                if remaining.starts_with(&['!', '!', '}']) {
                    tokens.push(AbsoluteToken {
                        line: line_u32,
                        start_char: col,
                        length: 3,
                        token_type: TT_KEYWORD,
                        modifiers: 0,
                    });
                    i += 3;
                    col += 3;
                    continue;
                }

                // {{ echo }} — but not {{{
                if remaining.starts_with(&['{', '{']) && !remaining.starts_with(&['{', '{', '{']) {
                    tokens.push(AbsoluteToken {
                        line: line_u32,
                        start_char: col,
                        length: 2,
                        token_type: TT_KEYWORD,
                        modifiers: 0,
                    });
                    i += 2;
                    col += 2;
                    continue;
                }

                // }} closing echo — but not }}}
                if remaining.starts_with(&['}', '}']) && (i == 0 || chars[i - 1] != '}') {
                    tokens.push(AbsoluteToken {
                        line: line_u32,
                        start_char: col,
                        length: 2,
                        token_type: TT_KEYWORD,
                        modifiers: 0,
                    });
                    i += 2;
                    col += 2;
                    continue;
                }

                // @directive
                if chars[i] == '@' && i + 1 < chars.len() && chars[i + 1].is_alphabetic() {
                    let rest: String = chars[i + 1..].iter().collect();
                    if let Some(directive) = crate::blade::directives::match_directive(&rest) {
                        let token_len = 1 + directive.len() as u32; // @ + directive name
                        tokens.push(AbsoluteToken {
                            line: line_u32,
                            start_char: col,
                            length: token_len,
                            token_type: TT_KEYWORD,
                            modifiers: 0,
                        });
                        i += token_len as usize;
                        col += token_len;
                        continue;
                    }
                }

                col += chars[i].len_utf16() as u32;
                i += 1;
            }
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

/// Collect modifier bits (deprecated, static, readonly) for a member that
/// is known to exist on the fully-resolved class.
fn member_extra_modifiers(
    class: &ClassInfo,
    member_name: &str,
    is_static: bool,
    is_method_call: bool,
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
        // Constants first (most common in `Class::NAME` usage), then
        // static properties (`Class::$prop`).
        if let Some(c) = class.constants.iter().find(|c| c.name == member_name) {
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

/// Find a character subsequence starting from position `start` in a char slice.
fn find_substr(chars: &[char], start: usize, needle: &[char]) -> Option<usize> {
    if needle.is_empty() || start + needle.len() > chars.len() {
        return None;
    }
    for i in start..=chars.len() - needle.len() {
        if chars[i..i + needle.len()] == *needle {
            return Some(i);
        }
    }
    None
}

/// Compute the UTF-16 column of position `pos` in a char slice.
fn utf16_col_at(chars: &[char], pos: usize) -> u32 {
    chars[..pos].iter().map(|c| c.len_utf16() as u32).sum()
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
    line_index: &crate::util::LineIndex,
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

    #[test]
    fn test_blade_interpolation_alignment() {
        let backend = Backend::new_test();
        let uri = "file:///test.blade.php";
        // Blade: src="{{ \App\Library\MyImage::get('foo.png') }}"
        // Index: 012345678
        // \ is at col 8.
        let content = r#"src="{{ \App\Library\MyImage::get('foo.png') }}""#;

        backend.update_ast(uri, content);
        let res = backend.handle_semantic_tokens_full(uri, content).unwrap();
        let tokens = match res {
            tower_lsp::lsp_types::SemanticTokensResult::Tokens(tokens) => tokens,
            _ => panic!("Expected tokens"),
        };

        let mut line = 0u32;
        let mut start_char = 0u32;
        let mut found = None;

        for tok in tokens.data {
            if tok.delta_line > 0 {
                line += tok.delta_line;
                start_char = tok.delta_start;
            } else {
                start_char += tok.delta_start;
            }

            if tok.length == 20 {
                // App\Library\MyImage
                found = Some(AbsoluteToken {
                    line,
                    start_char,
                    length: tok.length,
                    token_type: tok.token_type,
                    modifiers: tok.token_modifiers_bitset,
                });
                break;
            }
        }

        let app_lib = found.expect("Should find App-Library token");
        assert_eq!(app_lib.line, 0);
        assert_eq!(app_lib.start_char, 8);
    }

    #[test]
    fn test_blade_foreach_alignment() {
        let backend = Backend::new_test();
        let uri = "file:///test.blade.php";
        // Blade: @foreach ($items as $item)
        // Col:    01234567890
        // $items starts at 10.
        let content = "@foreach ($items as $item)\n@endforeach";

        backend.update_ast(uri, content);
        let res = backend.handle_semantic_tokens_full(uri, content).unwrap();
        let tokens = match res {
            tower_lsp::lsp_types::SemanticTokensResult::Tokens(tokens) => tokens,
            _ => panic!("Expected tokens"),
        };

        // Find $items (length 6)
        let mut line = 0u32;
        let mut start_char = 0u32;
        let mut found = None;

        for tok in tokens.data {
            if tok.delta_line > 0 {
                line += tok.delta_line;
                start_char = tok.delta_start;
            } else {
                start_char += tok.delta_start;
            }

            if tok.length == 6 {
                found = Some(AbsoluteToken {
                    line,
                    start_char,
                    length: tok.length,
                    token_type: tok.token_type,
                    modifiers: tok.token_modifiers_bitset,
                });
                break;
            }
        }

        let items_tok = found.expect("Should find $items token");
        assert_eq!(items_tok.line, 0);
        assert_eq!(items_tok.start_char, 10); // "@foreach (" is 10 chars
    }
}
