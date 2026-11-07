//! Lower Tempest's expressions and element-scoped control flow into the
//! PHP consumed by the shared type engine.

use std::collections::HashMap;
use std::ops::Range;

use crate::template::source_map::TemplateSourceMap;

use super::syntax::{Token, tokens};

struct OpenTag<'a> {
    name: &'a str,
    blocks: usize,
    branch_depth: Option<usize>,
    isolated: bool,
}

/// Produce virtual PHP with exact mappings for every copied expression.
/// Attributes are evaluated inside the element's control flow, regardless
/// of where they are written in the opening tag.
pub(crate) fn preprocess(source: &str) -> (String, TemplateSourceMap) {
    preprocess_with_scope(source, None)
}

/// Lower inferred declarations beneath the template's explicit annotations.
pub(crate) fn preprocess_with_scope(
    source: &str,
    scope: Option<&super::inference::Scope>,
) -> (String, TemplateSourceMap) {
    let mut out = Output::new(source);
    if let Some(scope) = scope {
        out.scope(scope);
    }
    let mut stack: Vec<OpenTag<'_>> = Vec::new();
    let parsed = tokens(source);
    let forelse = forelse_pairs(&parsed);
    let mut carried_blocks = 0;
    for (index, token) in parsed.iter().enumerate() {
        let followed_by_branch = parsed.get(index + 1).is_some_and(|token| matches!(token, Token::Tag(tag) if !tag.closing && tag.attributes.iter().any(|attr| matches!(attr.name, ":else" | ":elseif"))));
        match token {
            Token::Php { code, echo, .. } => out.php_region(code.clone(), *echo),
            Token::Tag(tag) if tag.closing => {
                if let Some(index) = stack.iter().rposition(|open| open.name == tag.name) {
                    for open in stack.drain(index..).rev() {
                        if open.isolated {
                            out.insert("});\n");
                        }
                        let keep = if followed_by_branch && open.name == tag.name {
                            open.branch_depth.unwrap_or(0)
                        } else {
                            0
                        };
                        for _ in keep..open.blocks {
                            out.insert("}");
                        }
                        carried_blocks += keep;
                    }
                }
            }
            Token::Tag(tag) => {
                let mut blocks = std::mem::take(&mut carried_blocks);
                let mut branch_depth = None;
                let empty = forelse
                    .get(&tag.span.start)
                    .map(|start| format!("$__phpantom_tempest_empty_{start}"));
                if let Some(empty) = &empty
                    && tag.attributes.iter().any(|attr| attr.name == ":foreach")
                {
                    out.insert(&format!("{empty} = true;"));
                }
                // Tempest applies control flow around the whole element,
                // including attributes written before the directive.
                for attr in tag
                    .attributes
                    .iter()
                    .filter(|attr| is_control(attr.name))
                    .chain(tag.attributes.iter().filter(|attr| !is_control(attr.name)))
                {
                    if attr.name.starts_with("::") {
                        continue;
                    }
                    if !attr.name.starts_with(':') || attr.name.starts_with("::") {
                        if let Some(value) = &attr.value {
                            // Native PHP is also allowed in ordinary HTML
                            // attributes (class="<?= $active ? 'a' : 'b' ?>").
                            for token in tokens(&source[value.clone()]) {
                                if let Token::Php { code, echo, .. } = token {
                                    out.php_region(
                                        value.start + code.start..value.start + code.end,
                                        echo,
                                    );
                                }
                            }
                        }
                        continue;
                    }
                    match attr.name {
                        ":forelse" if empty.is_some() => {
                            out.insert(&format!("if ({}) {{", empty.as_deref().unwrap()));
                            blocks += 1;
                        }
                        ":else" => {
                            out.insert("else {");
                            branch_depth = Some(blocks);
                            blocks += 1;
                        }
                        ":if" | ":elseif" | ":isset" | ":foreach" => {
                            let Some(value) = &attr.value else {
                                continue;
                            };
                            let (prefix, suffix) = match attr.name {
                                ":foreach" => ("foreach (", ") {"),
                                ":elseif" => ("elseif (", ") {"),
                                _ => ("if (", ") {"),
                            };
                            out.insert(prefix);
                            if attr.name == ":isset" {
                                out.isset(value.clone());
                            } else {
                                out.copy(value.clone());
                            }
                            out.insert(suffix);
                            if attr.name == ":foreach"
                                && let Some(empty) = &empty
                            {
                                out.insert(&format!("{empty} = false;"));
                            }
                            if attr.name != ":foreach" {
                                branch_depth = Some(blocks);
                            }
                            blocks += 1;
                        }
                        _ => {
                            if let Some(value) = &attr.value {
                                // An attribute can carry any PHP value,
                                // including objects passed to components.
                                // An expression statement preserves its type
                                // without imposing echo's scalar constraint.
                                out.copy(value.clone());
                                out.insert(";");
                            }
                        }
                    }
                }
                // Zero-width anchors locate a component call or slot outlet
                // in its PHP scope without pretending its HTML is PHP code.
                if tag.name.starts_with("x-") {
                    out.insert("\n ");
                    out.copy(tag.span.start..tag.span.start);
                    out.insert(" \n");
                }
                if let Some(names) = scope.and_then(|scope| scope.slot_reads.get(&tag.span.start)) {
                    for name in names {
                        out.insert(&format!("${name};\n"));
                    }
                }
                let isolated = !tag.self_closing
                    && scope.is_some_and(|scope| scope.regions.contains_key(&tag.span.start));
                if isolated {
                    out.insert("(function () {\n");
                    out.scope(&scope.unwrap().regions[&tag.span.start]);
                }
                if tag.self_closing {
                    let keep = if followed_by_branch {
                        branch_depth.unwrap_or(0)
                    } else {
                        0
                    };
                    for _ in keep..blocks {
                        out.insert("}");
                    }
                    carried_blocks = keep;
                } else {
                    stack.push(OpenTag {
                        name: tag.name,
                        blocks,
                        branch_depth,
                        isolated,
                    });
                }
            }
        }
    }
    for open in stack.into_iter().rev() {
        if open.isolated {
            out.insert("});\n");
        }
        for _ in 0..open.blocks {
            out.insert("}");
        }
    }
    out.insert("\n}\n");
    out.spans.finish();
    (
        out.php,
        TemplateSourceMap {
            copied: Some(Box::new(out.spans)),
            template_len: source.len(),
            prologue_lines: 2,
            ..TemplateSourceMap::default()
        },
    )
}

/// Pair a loop with its following `:forelse` sibling. Only those loops
/// need an empty flag, so ordinary foreach bodies introduce no extra data.
fn forelse_pairs(tokens: &[Token<'_>]) -> HashMap<usize, usize> {
    let mut stack: Vec<usize> = Vec::new();
    let mut pairs = HashMap::new();
    for (index, token) in tokens.iter().enumerate() {
        let Token::Tag(tag) = token else {
            continue;
        };
        let opening = if tag.closing {
            let Some(at) = stack
                .iter()
                .rposition(|&open| matches!(&tokens[open], Token::Tag(t) if t.name == tag.name))
            else {
                continue;
            };
            let open = stack[at];
            stack.truncate(at);
            open
        } else if tag.self_closing {
            index
        } else {
            stack.push(index);
            continue;
        };
        if let Token::Tag(open) = &tokens[opening]
            && open.attributes.iter().any(|attr| attr.name == ":foreach")
            && let Some(Token::Tag(next)) = tokens.get(index + 1)
            && !next.closing
            && next.attributes.iter().any(|attr| attr.name == ":forelse")
        {
            pairs.insert(open.span.start, open.span.start);
            pairs.insert(next.span.start, open.span.start);
        }
    }
    pairs
}

fn is_control(name: &str) -> bool {
    matches!(
        name,
        ":if" | ":elseif" | ":else" | ":isset" | ":foreach" | ":forelse"
    )
}

struct Output<'a> {
    source: &'a str,
    lines: crate::text_position::LineIndex<'a>,
    position: tower_lsp_server::gen_lsp_types::Position,
    php: String,
    spans: crate::template::source_spans::SourceSpans,
}

fn annotation_type(ty: &crate::php_type::PhpType) -> String {
    let rendered = ty.to_reparseable_string();
    if !rendered.contains("*/") && !rendered.contains(['\r', '\n']) {
        return rendered;
    }
    // Both literal values and shape keys can contain a comment terminator.
    // Re-encode only those PHP strings, retaining their exact runtime value.
    let bytes = rendered.as_bytes();
    let mut out = String::with_capacity(rendered.len());
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        if !matches!(bytes[at], b'\'' | b'"') {
            at += 1;
            continue;
        }
        let end = crate::text_scan::skip_string_forward(bytes, at);
        let raw = &rendered[at..end];
        if raw.contains("*/") || raw.contains(['\r', '\n']) {
            let literal = crate::php_type::LiteralValue::string_raw(raw);
            let value = literal.string_content().unwrap();
            let escaped = value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('/', "\\x2f")
                .replace('\r', "\\r")
                .replace('\n', "\\n");
            out.push_str(&rendered[copied..at]);
            out.push('"');
            out.push_str(&escaped);
            out.push('"');
            copied = end;
        }
        at = end;
    }
    out.push_str(&rendered[copied..]);
    out
}

impl<'a> Output<'a> {
    fn scope(&mut self, scope: &super::inference::Scope) {
        for (name, ty) in &scope.vars {
            self.declare(name, ty, true);
        }
        if let Some(ty) = scope.this_type() {
            self.declare("this", &ty, false);
        }
    }

    fn declare(&mut self, name: &str, ty: &crate::php_type::PhpType, assign: bool) {
        // A literal string type can itself contain a comment terminator.
        // It must not terminate an injected annotation.
        let ty = annotation_type(ty);
        self.insert(&format!("/** @var {ty} ${name} */\n"));
        if assign {
            self.insert(&format!("${name} = null;\n"));
        }
    }

    fn new(source: &'a str) -> Self {
        let mut out = Self {
            source,
            lines: crate::text_position::LineIndex::new(source),
            position: Default::default(),
            php: String::with_capacity(source.len()),
            spans: Default::default(),
        };
        out.insert("<?php\nfunction ");
        out.insert(crate::blade::WRAPPER_FUNCTION);
        out.insert("() {\n");
        out
    }

    fn insert(&mut self, text: &str) {
        self.php.push_str(text);
        for ch in text.chars() {
            if ch == '\n' {
                self.position.line += 1;
                self.position.character = 0;
            } else {
                self.position.character += ch.len_utf16() as u32;
            }
        }
    }

    fn copy(&mut self, range: Range<usize>) {
        let start = self.position;
        self.insert(&self.source[range.clone()]);
        self.spans.push(
            tower_lsp_server::gen_lsp_types::Range {
                start: self.lines.position(range.start),
                end: self.lines.position(range.end),
            },
            tower_lsp_server::gen_lsp_types::Range {
                start,
                end: self.position,
            },
        );
    }

    fn php_region(&mut self, code: Range<usize>, echo: bool) {
        if echo {
            self.insert("echo ");
        }
        self.copy(code.clone());
        // PHP's closing tag terminates a statement and even a // comment.
        // Keep standalone docblocks adjacent to the following statement.
        self.insert("\n");
        let bytes = &self.source.as_bytes()[code];
        let last = super::syntax::last_php_token(bytes);
        if echo || last.is_some_and(|last| !matches!(last, b';' | b'{' | b'}' | b':')) {
            self.insert(";\n");
        }
    }

    fn isset(&mut self, range: Range<usize>) {
        let bytes = self.source.as_bytes();
        let end = range.end;
        let mut copied = range.start;
        let mut i = range.start;
        while i < end {
            if bytes[i] != b'$' {
                i += 1;
                continue;
            }
            let start = i;
            i += 1;
            while i < end
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 128)
            {
                i += 1;
            }
            loop {
                if bytes.get(i..i + 2) == Some(b"->") {
                    i += 2;
                    while i < end
                        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] >= 128)
                    {
                        i += 1;
                    }
                } else if bytes.get(i) == Some(&b'[')
                    && let Some(close) =
                        crate::text_scan::find_matching_forward_bytes(bytes, i, b'[', b']')
                    && close < end
                {
                    i = close + 1;
                } else {
                    break;
                }
            }
            if copied < start {
                self.copy(copied..start);
            }
            self.insert("isset(");
            self.copy(start..i);
            self.insert(")");
            copied = i;
        }
        if copied < end {
            self.copy(copied..end);
        }
    }
}
