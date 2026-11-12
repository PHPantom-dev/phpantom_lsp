//! Named view data completes and navigates through the receiving template's PHP.

use std::collections::{BTreeMap, HashMap};

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::{Atom, atom};
use crate::completion::named_args::NamedArgContext;
use crate::symbol_map::{SymbolKind, VarDefKind};
use crate::text_position::{LineIndex, byte_range_to_lsp_range, position_to_offset};
use crate::types::FileContext;

use super::view_data::{call_target, is_view_function};

#[derive(Clone, Copy)]
struct Read {
    /// PHP coordinates for the regular definition and hover pipelines.
    range: Range,
    /// Source order can differ from PHP order when directives reorder attributes.
    source: Position,
}

impl Backend {
    fn tempest_view_input_reads(&self, uri: &str, php: &str) -> BTreeMap<Atom, Read> {
        let Some(map) = self
            .symbol_map_for(uri)
            .filter(|map| map.source(php).is_some())
        else {
            return BTreeMap::new();
        };
        let lines = LineIndex::new(php);
        // Tempest lowers the template into a generated wrapper function.
        let root_scope = if self.is_tempest_view(uri) {
            map.scopes.first().map_or(0, |&(start, _)| start)
        } else {
            0
        };
        let range = |start: u32, end: u32| Range {
            start: lines.position(start as usize),
            end: lines.position(end as usize),
        };
        // Injected type bindings have no source range and do not make data local.
        let definitions = map.var_defs.iter().filter(|definition| {
            definition.scope_start == root_scope
                && !matches!(
                    definition.kind,
                    VarDefKind::DocblockVar
                        | VarDefKind::DocblockParam
                        | VarDefKind::Property
                        | VarDefKind::CompoundAssignment
                        | VarDefKind::ClosureCapture
                )
                && self
                    .try_translate_template_range(
                        uri,
                        range(
                            definition.offset,
                            definition.offset + definition.name.len() as u32 + 1,
                        ),
                    )
                    .is_some()
        });
        let mut bindings = Vec::new();
        for definition in definitions {
            bindings.push((definition.effective_from, definition.name.as_str(), true));
            if definition.nesting_depth > 0 && definition.block_end < u32::MAX {
                bindings.push((definition.block_end + 1, definition.name.as_str(), false));
            }
        }
        bindings.sort_unstable_by_key(|&(offset, _, _)| offset);
        let mut bindings = bindings.into_iter().peekable();
        let mut locals = HashMap::new();
        let mut reads: BTreeMap<Atom, Read> = BTreeMap::new();
        for span in &map.spans {
            while bindings
                .peek()
                .is_some_and(|&(offset, _, _)| offset <= span.start)
            {
                let (_, name, active) = bindings.next().unwrap();
                let count = locals.entry(name).or_insert(0usize);
                if active {
                    *count += 1;
                } else {
                    *count -= 1;
                }
            }
            let SymbolKind::Variable { name } = &span.kind else {
                continue;
            };
            if !super::inference::variable_name(name)
                || crate::diagnostics::helpers::SUPERGLOBALS
                    .iter()
                    .any(|global| global.strip_prefix('$') == Some(name))
                || locals.get(name.as_str()).is_some_and(|&count| count > 0)
                || !input_scope(&map, name, span.start, root_scope)
                || map.var_def_kind_at(name, span.start).is_some_and(|kind| {
                    !matches!(
                        kind,
                        VarDefKind::CompoundAssignment | VarDefKind::ClosureCapture
                    )
                })
            {
                continue;
            }
            let php_range = range(span.start, span.end);
            let Some(source) = self.try_translate_template_range(uri, php_range) else {
                continue;
            };
            let read = Read {
                range: php_range,
                source: source.start,
            };
            let previous = reads.entry(*name).or_insert(read);
            if (read.source.line, read.source.character)
                < (previous.source.line, previous.source.character)
            {
                *previous = read;
            }
        }
        reads
    }

    /// Complete template inputs the call has not already supplied.
    pub(crate) fn tempest_view_named_completions(
        &self,
        uri: &str,
        context: &NamedArgContext,
        content: &str,
        position: Position,
        file: &FileContext,
    ) -> Option<Vec<CompletionItem>> {
        let offset = position_to_offset(content, position);
        if !self.is_tempest_project() || !is_view_function(&context.call_expression, file, offset) {
            return None;
        }
        let start =
            (offset as usize).checked_sub(context.prefix.len() + context.args_text.len())?;
        let target = call_target(self, uri, content, file, start as u32).or_else(|| {
            // Preserve byte offsets, including CRLF, when a trailing call is incomplete.
            let patched = format!(
                "{});",
                content.get(..offset as usize - context.prefix.len())?
            );
            call_target(self, uri, &patched, file, start as u32)
        })?;
        self.with_file_content("tempest_view_parameters", &target.uri, None, |php, _| {
            self.tempest_view_input_reads(&target.uri, php)
                .into_keys()
                .filter(|name| {
                    name != "path"
                        && name.starts_with(&context.prefix)
                        && !target.supplied.contains(name.as_str())
                })
                .map(|name| CompletionItem {
                    label: format!("{name}:"),
                    kind: Some(CompletionItemKind::Variable),
                    detail: Some("View parameter".to_owned()),
                    insert_text: Some(format!("{name}: ")),
                    ..Default::default()
                })
                .collect()
        })
    }

    fn tempest_view_argument_target(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<(String, Atom, Range)> {
        if !self.is_tempest_project() {
            return None;
        }
        let offset = position_to_offset(content, position);
        let map = self
            .symbol_map_for(uri)
            .filter(|map| map.source(content).is_some())?;
        let call = map.find_enclosing_call_site(offset)?;
        let name = call
            .named_arg_indices
            .iter()
            .zip(call.named_arg_names.iter())
            .find_map(|(&index, name)| {
                let start = call.arg_offsets[index as usize];
                (name != "path"
                    && start <= offset
                    && offset <= start + name.len() as u32
                    && content.get(start as usize..start as usize + name.len()) == Some(name))
                .then_some((name, start))
            })?;
        let file = self.file_context(uri);
        if !is_view_function(call.call_expression, &file, offset) {
            return None;
        }
        let target = call_target(self, uri, content, &file, call.args_start)?;
        Some((
            target.uri,
            atom(name.0),
            byte_range_to_lsp_range(content, name.1 as usize, name.1 as usize + name.0.len()),
        ))
    }

    /// Navigate a data label to its first read in the target template.
    pub(crate) fn tempest_view_argument_definition(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Location> {
        let (target, name, _) = self.tempest_view_argument_target(uri, content, position)?;
        self.with_file_content(
            "tempest_view_parameter_definition",
            &target,
            None,
            |php, _| {
                Some(Location {
                    uri: crate::util::parse_url(&target)?,
                    range: self
                        .tempest_view_input_reads(&target, php)
                        .get(&name)?
                        .range,
                })
            },
        )
        .flatten()
    }

    /// Show a view parameter with the same type its template read has.
    pub(crate) fn tempest_view_argument_hover(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Hover> {
        let (target, name, range) = self.tempest_view_argument_target(uri, content, position)?;
        self.with_file_content("tempest_view_parameter_hover", &target, None, |php, _| {
            let read = *self.tempest_view_input_reads(&target, php).get(&name)?;
            let mut contents = format!("**View parameter** `${name}`");
            if let Some(Hover {
                contents: Contents::MarkupContent(typed),
                ..
            }) = self.handle_hover(&target, php, read.range.start)
            {
                contents.push_str("\n\n");
                contents.push_str(&typed.value);
            }
            let mut hover = crate::hover::formatting::make_hover(contents);
            hover.range = Some(range);
            Some(hover)
        })
        .flatten()
    }
}

fn input_scope(map: &crate::symbol_map::SymbolMap, name: &str, offset: u32, root: u32) -> bool {
    let mut scope = map.find_enclosing_scope(offset);
    while scope != root && map.is_arrow_fn_scope(scope) {
        if map.find_var_definition(name, offset, scope).is_some() {
            return false;
        }
        scope = map.find_enclosing_scope(scope - 1);
    }
    scope == root
}

#[cfg(test)]
mod tests;
