//! The `@template` parameters and type aliases a file's docblocks
//! declare, and the declarations each one reaches.  A written docblock
//! name that one of them declares keeps its spelling instead of being
//! qualified with the current namespace.

use std::collections::HashMap;

/// The template parameters and type aliases a file's docblocks declare,
/// each with the spans of the declarations that put it in scope.
#[derive(Default)]
pub(crate) struct DocblockTypeNames(HashMap<String, Vec<(u32, u32)>>);

impl DocblockTypeNames {
    /// Collect every declaring docblock of `content` and the class,
    /// function, method, or closure it documents.  A docblock attached to
    /// none of those declares its names for the whole file.
    pub(crate) fn collect(content: &str) -> Self {
        let mut declared: Vec<(&str, usize)> = Vec::new();
        let bytes = content.as_bytes();
        for start in memchr::memmem::find_iter(bytes, b"/**") {
            // From the opening `*`, so an empty `/**/` closes on itself.
            let Some(len) = memchr::memmem::find(&bytes[start + 2..], b"*/") else {
                break;
            };
            let end = start + 2 + len + 2;
            for line in content[start..end].lines().filter(|l| l.contains('@')) {
                line_declared_type_names(line, |name| declared.push((name, end)));
            }
        }
        let mut names = HashMap::new();
        if declared.is_empty() {
            return Self(names);
        }

        let spans = declaration_spans(content);
        for (name, doc_end) in declared {
            let scope = spans
                .get(spans.partition_point(|&(start, _)| (start as usize) < doc_end))
                .filter(|&&(start, _)| {
                    let between = &content[doc_end..start as usize];
                    !between.contains([';', '{', '}'])
                })
                .copied()
                .unwrap_or((0, u32::MAX));
            names
                .entry(name.to_string())
                .or_insert_with(Vec::new)
                .push(scope);
        }
        Self(names)
    }

    /// Whether `name` is declared on a declaration overlapping `region`.
    pub(crate) fn declares(&self, name: &str, region: (u32, u32)) -> bool {
        self.0.get(name).is_some_and(|scopes| {
            scopes
                .iter()
                .any(|&(start, end)| start <= region.1 && region.0 <= end)
        })
    }
}

/// The spans of `content`'s classes, functions, methods, and closures,
/// sorted by start.
fn declaration_spans(content: &str) -> Vec<(u32, u32)> {
    use mago_span::HasSpan;
    use mago_syntax::cst::*;
    use mago_syntax::walker::Walker;

    struct Spans;
    fn push(spans: &mut Vec<(u32, u32)>, node: &impl HasSpan) {
        let span = node.span();
        spans.push((span.start.offset, span.end.offset));
    }
    impl<'ast, 'arena> Walker<'ast, 'arena, Vec<(u32, u32)>> for Spans {
        fn walk_in_class(&self, node: &'ast Class<'arena>, spans: &mut Vec<(u32, u32)>) {
            push(spans, node);
        }
        fn walk_in_interface(&self, node: &'ast Interface<'arena>, spans: &mut Vec<(u32, u32)>) {
            push(spans, node);
        }
        fn walk_in_trait(&self, node: &'ast Trait<'arena>, spans: &mut Vec<(u32, u32)>) {
            push(spans, node);
        }
        fn walk_in_enum(&self, node: &'ast Enum<'arena>, spans: &mut Vec<(u32, u32)>) {
            push(spans, node);
        }
        fn walk_in_anonymous_class(
            &self,
            node: &'ast AnonymousClass<'arena>,
            spans: &mut Vec<(u32, u32)>,
        ) {
            push(spans, node);
        }
        fn walk_in_function(&self, node: &'ast Function<'arena>, spans: &mut Vec<(u32, u32)>) {
            push(spans, node);
        }
        fn walk_in_method(&self, node: &'ast Method<'arena>, spans: &mut Vec<(u32, u32)>) {
            push(spans, node);
        }
        fn walk_in_closure(&self, node: &'ast Closure<'arena>, spans: &mut Vec<(u32, u32)>) {
            push(spans, node);
        }
        fn walk_in_arrow_function(
            &self,
            node: &'ast ArrowFunction<'arena>,
            spans: &mut Vec<(u32, u32)>,
        ) {
            push(spans, node);
        }
    }

    let mut spans =
        crate::parser::with_parsed_program(content, "declaration_spans", |program, _| {
            let mut spans = Vec::new();
            for statement in program.statements.iter() {
                Spans.walk_statement(statement, &mut spans);
            }
            spans
        });
    spans.sort_unstable();
    spans
}

/// Whether any docblock of `content` declares `name`, in or out of scope:
/// a search for `name` alone, cheap enough to rule a name out before
/// [`DocblockTypeNames::collect`] parses the file for scopes.
pub(crate) fn scan_declares_docblock_type_name(content: &str, name: &str) -> bool {
    let bytes = content.as_bytes();
    memchr::memmem::find_iter(bytes, name.as_bytes()).any(|pos| {
        // A declared name follows the tag (or `as`) after whitespace.
        if pos == 0 || !bytes[pos - 1].is_ascii_whitespace() {
            return false;
        }
        let line_start = content[..pos].rfind('\n').map_or(0, |i| i + 1);
        let line_end = content[pos..].find('\n').map_or(content.len(), |i| pos + i);
        let mut found = false;
        line_declared_type_names(&content[line_start..line_end], |declared| {
            found |= declared == name;
        });
        found
    })
}

fn line_declared_type_names<'a>(line: &'a str, mut found: impl FnMut(&'a str)) {
    let identifier = |word: &'a str| {
        let end = word
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || !c.is_ascii()))
            .unwrap_or(word.len());
        &word[..end]
    };
    for tag in line.split('@').skip(1) {
        let (tag, rest) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let mut words = rest.split_whitespace();
        if tag.ends_with("import-type") {
            // `@phpstan-import-type Alias from Source as Local`
            if let Some(alias) = words.next() {
                found(identifier(alias));
            }
            if let Some(local) = words.skip_while(|w| *w != "as").nth(1) {
                found(identifier(local));
            }
        } else if (tag.ends_with("template")
            || tag.ends_with("template-covariant")
            || tag.ends_with("template-contravariant")
            || tag == "type"
            || tag.ends_with("-type"))
            && let Some(name) = words.next()
        {
            found(identifier(name));
        }
    }
}
