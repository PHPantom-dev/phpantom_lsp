//! Completion and navigation for Tempest's PHP view-path arguments.

use std::collections::BTreeSet;

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::completion::source::code_context::CodeContext;
use crate::text_position::{byte_range_to_lsp_range, position_to_offset};
use crate::types::FileContext;

impl Backend {
    pub(crate) fn tempest_view_references(
        &self,
        uri: &str,
        name: &str,
        relative: bool,
        snapshot: &[(String, std::sync::Arc<crate::symbol_map::SymbolMap>)],
        include_declaration: bool,
    ) -> Vec<Location> {
        use crate::symbol_map::{StringKeyKind, SymbolKind};
        let Some(target) = self.tempest_view_definition(uri, name, relative) else {
            return Vec::new();
        };
        let mut locations = Vec::new();
        for (file_uri, map) in snapshot {
            let Some(parsed_uri) = crate::util::parse_url(file_uri) else {
                continue;
            };
            let mut content = None;
            for span in &map.spans {
                let SymbolKind::StringKey {
                    kind: StringKeyKind::TempestView { relative },
                    key,
                    ..
                } = &span.kind
                else {
                    continue;
                };
                if !self
                    .tempest_view_definition(file_uri, key, *relative)
                    .is_some_and(|definition| definition.uri == target.uri)
                {
                    continue;
                }
                let Some(content) = content
                    .get_or_insert_with(|| self.reference_file_content_arc(file_uri))
                    .as_ref()
                else {
                    continue;
                };
                locations.push(Location {
                    uri: parsed_uri.clone(),
                    range: byte_range_to_lsp_range(content, span.start as usize, span.end as usize),
                });
            }
        }
        if include_declaration {
            locations.push(target);
        }
        locations
    }

    pub(crate) fn tempest_view_definition(
        &self,
        uri: &str,
        name: &str,
        relative: bool,
    ) -> Option<Location> {
        let path = if relative {
            let caller = crate::util::uri_to_path(uri)?;
            let mut path = caller.parent()?.as_os_str().to_os_string();
            path.push(name);
            std::path::PathBuf::from(path)
        } else {
            self.tempest_view_file(uri, name)?
        };
        if !path.is_file()
            && !self
                .open_files
                .read()
                .contains_key(&crate::util::path_to_uri(&path))
        {
            return None;
        }
        let path = dunce::canonicalize(&path).unwrap_or(path);
        Some(Location {
            uri: crate::util::parse_url(&crate::util::path_to_uri(&path))?,
            range: Range::default(),
        })
    }

    pub(crate) fn tempest_path_completion(
        &self,
        uri: &str,
        content: &str,
        position: Position,
        ctx: &FileContext,
        code: &CodeContext<'_>,
    ) -> Option<CompletionResponse> {
        let (quote, _) = code.open_string?;
        let call = code.open_brackets.last()?;
        if call.byte != b'(' || call.callee_operator.is_some() {
            return None;
        }
        let name_start = content[..call.code_before]
            .bytes()
            .rposition(|byte| {
                !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\') || byte >= 128)
            })
            .map_or(0, |at| at + 1);
        let written = content[name_start..call.code_before].trim();
        let (resolved, _) = crate::names::function_lookup_names(
            written,
            &ctx.use_map,
            &ctx.symbol_imports.functions,
            &ctx.namespace,
        );
        if !resolved
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("Tempest\\View\\view")
        {
            return None;
        }
        let before = code.code_before.trim_end();
        let before_dir = before
            .strip_suffix('.')
            .map(str::trim_end)
            .and_then(|before| before.strip_suffix("__DIR__"));
        let relative = before_dir.is_some();
        let before = before_dir.unwrap_or(before).trim_end();
        let named = before
            .strip_suffix(':')
            .map(str::trim_end)
            .is_some_and(|before| {
                before.ends_with("path")
                    && before[..before.len() - 4].trim_end().ends_with(['(', ','])
            });
        if !named && !(call.commas == 0 && before.ends_with('(')) {
            return None;
        }
        let offset = position_to_offset(content, position) as usize;
        let prefix = content.get(quote + 1..offset)?;
        if prefix.contains(['$', '{', '\\']) {
            return None;
        }
        let caller = crate::util::uri_to_path(uri)?;
        let caller_dir = caller.parent()?;
        let caller_dir =
            dunce::canonicalize(caller_dir).unwrap_or_else(|_| caller_dir.to_path_buf());
        let catalog = self.tempest_catalog();
        let workspace = self
            .workspace
            .workspace_root
            .read()
            .clone()
            .and_then(|root| dunce::canonicalize(root).ok());
        let mut names = BTreeSet::new();
        for file in &catalog.files {
            if let Ok(path) = file.strip_prefix(&caller_dir) {
                let name = path.to_string_lossy().replace('\\', "/");
                names.insert(if relative {
                    format!("/{name}")
                } else if prefix.starts_with("./") {
                    format!("./{name}")
                } else {
                    name
                });
            }
            if !relative {
                for root in catalog.roots.iter().chain(workspace.iter()) {
                    if let Ok(path) = file.strip_prefix(root) {
                        names.insert(path.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
        let end =
            crate::text_scan::skip_string_forward(content.as_bytes(), quote).min(content.len());
        let end = if content.as_bytes().get(end.saturating_sub(1)) == content.as_bytes().get(quote)
        {
            end - 1
        } else {
            offset
        };
        let range = byte_range_to_lsp_range(content, quote + 1, end);
        Some(CompletionResponse::CompletionItemList(
            names
                .into_iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::File),
                    detail: Some("Tempest view".to_string()),
                    text_edit: Some(CompletionItemTextEdit::TextEdit(TextEdit {
                        range,
                        new_text: name,
                    })),
                    ..Default::default()
                })
                .collect(),
        ))
    }
}
