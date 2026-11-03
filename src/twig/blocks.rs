//! The blocks a Twig template defines, overrides, and renders.
//!
//! `{% block body %}` in a template that extends another overrides the
//! parent's block of the same name, however many `extends` up it was first
//! defined, and `{{ block('body') }}` and `{{ parent() }}` render one.  A
//! template pulls in the blocks of the ones it `use`s as well (under another
//! name when the tag says `with body as base_body`), and the blocks inside an
//! `{% embed %}` override the embedded template's, not the parent's.
//!
//! This scan reads one template's source and records every block name it
//! writes, with the scope the name belongs to: the template itself, or one
//! of its embeds, each of which extends the template it embeds.  Pairing the
//! names up across templates is [`super::block_navigation`]'s.

use std::ops::Range;

use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolSpan};
use crate::symfony::translations::{for_each_twig_block, skip_whitespace, string_end};

/// What a template does with a block name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockRole {
    /// `{% block body %}`, and the name an `{% endblock body %}` repeats.
    Define,
    /// `block('body')`, which renders the block.
    Render,
    /// `parent()`, which renders the block the enclosing one overrides.
    Parent,
}

/// One block name as a template writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockRef {
    pub(crate) name: String,
    pub(crate) role: BlockRole,
    /// The name's bytes, inside the quotes of a `block()`, or the `parent`
    /// of a `parent()`.
    pub(crate) span: Range<usize>,
    /// The index of the scope in [`TemplateBlocks::scopes`] it is written
    /// in.
    pub(crate) scope: usize,
    /// The template a `block('body', 'other.html.twig')` renders the block
    /// of, which it is looked up in rather than in the scope.
    pub(crate) template: Option<String>,
}

/// A template a scope pulls blocks in from with `use`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UsedTemplate {
    pub(crate) template: String,
    /// The `with body as base_body` renames, as `(body, base_body)`: the
    /// block is pulled in under the new name only.
    pub(crate) aliases: Vec<(String, String)>,
}

/// A template, or one embed inside it: something whose blocks override
/// those of the templates it extends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Scope {
    /// The templates it extends, each a candidate when the name is chosen
    /// at runtime (`{% extends ajax ? 'bare.html.twig' : 'base.html.twig' %}`).
    pub(crate) parents: Vec<String>,
    pub(crate) uses: Vec<UsedTemplate>,
    /// Where an embed's body lies; the whole template for the first scope.
    pub(crate) range: Range<usize>,
}

impl UsedTemplate {
    /// The name the block `name` has in the used template, when it is
    /// pulled in as `name` at all.
    pub(crate) fn source_name<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        if let Some((from, _)) = self.aliases.iter().find(|(_, to)| to == name) {
            return Some(from);
        }
        (!self.aliases.iter().any(|(from, _)| from == name)).then_some(name)
    }
}

/// Every block name a template writes, and the scopes they are written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TemplateBlocks {
    pub(crate) refs: Vec<BlockRef>,
    /// The template itself first, then each embed in the order it opens.
    pub(crate) scopes: Vec<Scope>,
}

impl TemplateBlocks {
    /// The innermost scope `offset` lies in.
    pub(crate) fn scope_at(&self, offset: usize) -> usize {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, scope)| scope.range.start <= offset && offset <= scope.range.end)
            .map_or(0, |(index, _)| index)
    }

    /// The name written at `offset`, the end of a name included.
    pub(crate) fn ref_at(&self, offset: usize) -> Option<&BlockRef> {
        self.refs
            .iter()
            .find(|block| block.span.start <= offset && offset <= block.span.end)
    }

    /// The names `scope` defines, in the order it defines them.
    pub(crate) fn defined_names(&self, scope: usize) -> impl Iterator<Item = &str> {
        self.refs
            .iter()
            .filter(move |block| block.scope == scope && block.role == BlockRole::Define)
            .map(|block| block.name.as_str())
    }
}

/// Read every block name `content` writes.
pub(crate) fn analyse(content: &str) -> TemplateBlocks {
    let bytes = content.as_bytes();
    let mut out = TemplateBlocks {
        refs: Vec::new(),
        scopes: vec![Scope {
            range: 0..content.len(),
            ..Scope::default()
        }],
    };
    if !content.contains("{%") && !content.contains("{{") {
        return out;
    }
    // The embeds that are open, innermost last.
    let mut embeds: Vec<usize> = Vec::new();
    // The blocks that are open, with the scope each was opened in, so a
    // `parent()` knows which block it is in.
    let mut open: Vec<(String, usize)> = Vec::new();

    for_each_twig_block(content, |body, end| {
        let scope = embeds.last().copied().unwrap_or(0);
        let mut calls_from = body;
        let mut closes_itself = false;
        if bytes[body - 1] == b'%'
            && let Some((tag, after)) = tag_name(content, body, end)
        {
            match tag {
                "block" => {
                    if let Some(span) =
                        identifier_at(content, skip_whitespace(bytes, after, end), end)
                    {
                        let name = content[span.clone()].to_string();
                        calls_from = span.end;
                        // `{% block title page.title %}` is the whole block;
                        // only the bare form waits for its `endblock`.
                        closes_itself =
                            skip_whitespace(bytes, span.end, end) < trim_end(bytes, body, end);
                        open.push((name.clone(), scope));
                        out.refs.push(BlockRef {
                            name,
                            role: BlockRole::Define,
                            span,
                            scope,
                            template: None,
                        });
                    }
                }
                "endblock" => {
                    if let Some((name, opened_in)) = open.pop()
                        && let Some(span) =
                            identifier_at(content, skip_whitespace(bytes, after, end), end)
                        && content[span.clone()] == name
                    {
                        out.refs.push(BlockRef {
                            name,
                            role: BlockRole::Define,
                            span,
                            scope: opened_in,
                            template: None,
                        });
                    }
                    return;
                }
                "extends" => {
                    let names = literal_names(content, after, end);
                    out.scopes[scope].parents.extend(names);
                    return;
                }
                "use" => {
                    if let Some(used) = used_template(content, after, end) {
                        out.scopes[scope].uses.push(used);
                    }
                    return;
                }
                "embed" => {
                    out.scopes.push(Scope {
                        parents: literal_names(content, after, end),
                        uses: Vec::new(),
                        range: end..content.len(),
                    });
                    embeds.push(out.scopes.len() - 1);
                    calls_from = after;
                }
                "endembed" => {
                    if let Some(index) = embeds.pop() {
                        out.scopes[index].range.end = body.saturating_sub(2);
                    }
                    // A block left open inside the embed closes with it.
                    open.retain(|(_, opened_in)| embeds.contains(opened_in) || *opened_in == 0);
                    return;
                }
                _ => {}
            }
        }
        push_calls(content, calls_from, end, scope, &open, &mut out.refs);
        if closes_itself {
            open.pop();
        }
    });
    out.refs.sort_by_key(|block| block.span.start);
    out
}

/// The spans a template's block names are recorded with in its symbol map.
pub(crate) fn twig_block_spans(content: &str) -> Vec<SymbolSpan> {
    analyse(content)
        .refs
        .into_iter()
        .map(|block| SymbolSpan {
            start: block.span.start as u32,
            end: block.span.end as u32,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::TwigBlock,
                key: block.name,
                is_write: block.role == BlockRole::Define,
                is_optional: false,
            },
        })
        .collect()
}

/// The tag a `{% … %}` block whose body spans `body..end` opens with, and
/// where its name ends.
fn tag_name(content: &str, body: usize, end: usize) -> Option<(&str, usize)> {
    let bytes = content.as_bytes();
    let mut start = body;
    // `{%-` and `{%~` trim the whitespace around the tag.
    if matches!(bytes.get(start), Some(b'-' | b'~')) {
        start += 1;
    }
    let span = identifier_at(content, skip_whitespace(bytes, start, end), end)?;
    Some((&content[span.clone()], span.end))
}

/// Where the body of a block that spans `body..end` ends, before a `-` or
/// `~` that trims the whitespace after it.
fn trim_end(bytes: &[u8], body: usize, end: usize) -> usize {
    let mut cursor = end;
    if cursor > body && matches!(bytes[cursor - 1], b'-' | b'~') {
        cursor -= 1;
    }
    while cursor > body && bytes[cursor - 1].is_ascii_whitespace() {
        cursor -= 1;
    }
    cursor
}

/// The identifier that starts at `start`, before `end`.
fn identifier_at(content: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let bytes = content.as_bytes();
    if !bytes
        .get(start)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        || start >= end
    {
        return None;
    }
    let length = bytes[start..end]
        .iter()
        .take_while(|byte| is_identifier_byte(**byte))
        .count();
    Some(start..start + length)
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The template names an `extends` or `embed` expression between `start`
/// and `end` spells: every string in it, up to the `with`, `only`, or
/// `ignore missing` that ends an embed's name.
fn literal_names(content: &str, start: usize, end: usize) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut names = Vec::new();
    let mut cursor = start;
    while cursor < end {
        match bytes[cursor] {
            quote @ (b'\'' | b'"') => {
                let Some(close) = string_end(bytes, cursor + 1, quote).filter(|c| *c <= end) else {
                    break;
                };
                names.push(content[cursor + 1..close].to_string());
                cursor = close + 1;
            }
            byte if is_identifier_byte(byte) => {
                let word = identifier_at(content, cursor, end).unwrap_or(cursor + 1..cursor + 1);
                if matches!(&content[word.clone()], "with" | "only" | "ignore") {
                    break;
                }
                cursor = word.end.max(cursor + 1);
            }
            _ => cursor += 1,
        }
    }
    names
}

/// The template a `use` tag between `start` and `end` names, with its
/// renames.
fn used_template(content: &str, start: usize, end: usize) -> Option<UsedTemplate> {
    let bytes = content.as_bytes();
    let cursor = skip_whitespace(bytes, start, end);
    let quote @ (b'\'' | b'"') = *bytes.get(cursor)? else {
        return None;
    };
    let close = string_end(bytes, cursor + 1, quote).filter(|close| *close <= end)?;
    let template = content[cursor + 1..close].to_string();
    let mut aliases = Vec::new();
    let rest = content[close + 1..end].trim();
    if let Some(list) = rest.strip_prefix("with") {
        for pair in list.split(',') {
            let mut words = pair.split_whitespace();
            if let (Some(from), Some("as"), Some(to)) = (words.next(), words.next(), words.next()) {
                aliases.push((from.to_string(), to.to_string()));
            }
        }
    }
    Some(UsedTemplate { template, aliases })
}

/// Push the `block()` and `parent()` calls between `start` and `end`.
fn push_calls(
    content: &str,
    start: usize,
    end: usize,
    scope: usize,
    open: &[(String, usize)],
    refs: &mut Vec<BlockRef>,
) {
    let bytes = content.as_bytes();
    let mut cursor = start;
    while cursor < end {
        let byte = bytes[cursor];
        if matches!(byte, b'\'' | b'"') {
            match string_end(bytes, cursor + 1, byte) {
                Some(close) => cursor = close + 1,
                None => return,
            }
            continue;
        }
        if !(byte.is_ascii_alphabetic() || byte == b'_') {
            cursor += 1;
            continue;
        }
        let word = identifier_at(content, cursor, end).unwrap_or(cursor..cursor + 1);
        cursor = word.end;
        // A method or attribute of something (`form.parent()`) is not the
        // function.
        let is_member = word.start > 0 && bytes[word.start - 1] == b'.';
        if is_member || bytes.get(word.end) != Some(&b'(') {
            continue;
        }
        match &content[word.clone()] {
            "parent" => {
                if let Some((name, opened_in)) = open.last() {
                    refs.push(BlockRef {
                        name: name.clone(),
                        role: BlockRole::Parent,
                        span: word,
                        scope: *opened_in,
                        template: None,
                    });
                }
            }
            "block" => {
                if let Some((block, after)) = block_call(content, word.end + 1, end, scope) {
                    refs.push(block);
                    cursor = after;
                }
            }
            _ => {}
        }
    }
}

/// The block a `block(` call whose arguments start at `start` renders,
/// when its name is a string, and where the call's literals end.  A second
/// argument names the template to look in, which must be a string to be
/// read.
fn block_call(content: &str, start: usize, end: usize, scope: usize) -> Option<(BlockRef, usize)> {
    let bytes = content.as_bytes();
    let (span, after) = string_at(bytes, skip_whitespace(bytes, start, end), end)?;
    let name = &content[span.clone()];
    if identifier_at(content, span.start, span.end)?.end != span.end {
        return None;
    }
    let mut cursor = skip_whitespace(bytes, after, end);
    let mut template = None;
    if bytes.get(cursor) == Some(&b',') {
        let (template_span, template_end) =
            string_at(bytes, skip_whitespace(bytes, cursor + 1, end), end)?;
        let close = skip_whitespace(bytes, template_end, end);
        if bytes.get(close) != Some(&b')') {
            return None;
        }
        template = Some(content[template_span].to_string());
        cursor = close;
    }
    Some((
        BlockRef {
            name: name.to_string(),
            role: BlockRole::Render,
            span,
            scope,
            template,
        },
        cursor,
    ))
}

/// The contents of the string literal that starts at `start`, and where it
/// ends.
fn string_at(bytes: &[u8], start: usize, end: usize) -> Option<(Range<usize>, usize)> {
    let quote @ (b'\'' | b'"') = *bytes.get(start)? else {
        return None;
    };
    let close = string_end(bytes, start + 1, quote).filter(|close| *close < end)?;
    Some((start + 1..close, close + 1))
}

/// Where the block name the cursor is typing starts, and whether it is
/// the name of a `{% block %}` tag (rather than of a `block()` call).
pub(crate) fn block_name_start(content: &str, cursor: usize) -> Option<(usize, bool)> {
    let before = content.get(..cursor)?;
    let bytes = before.as_bytes();
    let name_start = bytes
        .iter()
        .rposition(|byte| !is_identifier_byte(*byte))
        .map_or(0, |index| index + 1);
    let head = &before[..name_start];

    if let Some(quoted) = head.strip_suffix(['\'', '"']) {
        let call = quoted.trim_end().strip_suffix('(')?.trim_end();
        let word_start = call
            .bytes()
            .rposition(|byte| !is_identifier_byte(byte))
            .map_or(0, |index| index + 1);
        let is_call = &call[word_start..] == "block" && !call[..word_start].ends_with('.');
        let in_block =
            head.rfind("{{").max(head.rfind("{%")) > head.rfind("}}").max(head.rfind("%}"));
        return (is_call && in_block).then_some((name_start, false));
    }

    let tag = head.trim_end();
    if tag.len() == head.len() {
        return None;
    }
    let tag = tag.strip_suffix("block")?;
    let tag = tag.trim_end();
    let tag = tag
        .strip_suffix('-')
        .or_else(|| tag.strip_suffix('~'))
        .unwrap_or(tag);
    tag.ends_with("{%").then_some((name_start, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names `content` writes as short strings: `"define body 0"`, with
    /// the scope last.
    fn report(content: &str) -> Vec<String> {
        analyse(content)
            .refs
            .into_iter()
            .map(|block| {
                assert_eq!(
                    &content[block.span.clone()],
                    if block.role == BlockRole::Parent {
                        "parent"
                    } else {
                        block.name.as_str()
                    },
                    "the span must cover the name"
                );
                let role = match block.role {
                    BlockRole::Define => "define",
                    BlockRole::Render => "render",
                    BlockRole::Parent => "parent",
                };
                match block.template {
                    Some(template) => format!("{role} {} {} {template}", block.name, block.scope),
                    None => format!("{role} {} {}", block.name, block.scope),
                }
            })
            .collect()
    }

    #[test]
    fn blocks_their_ends_and_the_calls_that_render_them() {
        let twig = "{% extends 'base.html.twig' %}\n{% block title 'Home' %}\n{%- block body -%}\n  {{ parent() }}{{ block('title') }}{{ block(\"nav\", 'menu.html.twig') }}\n{% endblock body %}\n{# {% block commented %} #}{{ form.parent() }}{{ block(name) }}";
        assert_eq!(
            report(twig),
            [
                "define title 0",
                "define body 0",
                "parent body 0",
                "render title 0",
                "render nav 0 menu.html.twig",
                "define body 0",
            ]
        );
        assert_eq!(analyse(twig).scopes[0].parents, ["base.html.twig"]);
    }

    #[test]
    fn a_parent_outside_every_block_renders_nothing() {
        assert_eq!(
            report("{% block a parent() ~ '!' %}{{ parent() }}"),
            ["define a 0", "parent a 0"]
        );
    }

    #[test]
    fn the_blocks_of_an_embed_are_its_own() {
        let twig = "{% extends 'base.html.twig' %}{% block body %}{% embed 'card.html.twig' with {a: 'b.html.twig'} %}{% block content %}{{ parent() }}{% endblock %}{% endembed %}{{ parent() }}{% endblock %}";
        let blocks = analyse(twig);
        assert_eq!(
            report(twig),
            [
                "define body 0",
                "define content 1",
                "parent content 1",
                "parent body 0",
            ]
        );
        assert_eq!(blocks.scopes[1].parents, ["card.html.twig"]);
        let inside = twig.find("{% block content").unwrap();
        assert_eq!(blocks.scope_at(inside), 1);
        assert_eq!(blocks.scope_at(twig.rfind("parent").unwrap()), 0);
    }

    #[test]
    fn conditional_parents_are_all_candidates() {
        let blocks = analyse("{% extends ajax ? 'bare.html.twig' : 'base.html.twig' %}");
        assert_eq!(
            blocks.scopes[0].parents,
            ["bare.html.twig", "base.html.twig"]
        );
    }

    #[test]
    fn a_use_pulls_blocks_in_under_their_new_names() {
        let blocks = analyse(
            "{% use 'blocks.html.twig' with sidebar as base_sidebar, title as base_title %}",
        );
        let used = &blocks.scopes[0].uses[0];
        assert_eq!(used.template, "blocks.html.twig");
        assert_eq!(used.source_name("base_sidebar"), Some("sidebar"));
        assert_eq!(used.source_name("sidebar"), None);
        assert_eq!(used.source_name("footer"), Some("footer"));
    }

    #[test]
    fn completion_starts_at_the_name() {
        let at = |twig: &str| block_name_start(twig, twig.len());
        assert_eq!(at("{% block bo"), Some((9, true)));
        assert_eq!(at("{%- block "), Some((10, true)));
        assert_eq!(at("{{ block('ti"), Some((10, false)));
        assert_eq!(at("{% if block(\""), Some((13, false)));
        assert_eq!(at("{% endblock bo"), None);
        assert_eq!(at("{{ form.block('x"), None);
        assert_eq!(at("<p>block('x"), None);
        assert_eq!(at("{% block"), None);
    }
}
