//! Namespace rename edits.
//!
//! Handles `textDocument/rename` on a namespace segment: rewriting
//! `namespace` declarations, `use` statements, and inline FQN references
//! across every workspace file, and emitting `RenameFile` operations for
//! the PSR-4 directory move.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolKind;
use crate::text_position::{line_start_byte_offset, offset_to_position, ranges_overlap};
use crate::util::strip_fqn_prefix;

impl Backend {
    /// Build a `WorkspaceEdit` for renaming a namespace segment.
    ///
    /// `full_ns` is the full namespace at the declaration site (e.g.
    /// `"App\\Bar\\Service"`).  `segment_idx` is the 0-based index of
    /// the segment being renamed.  `new_segment` is the replacement
    /// text for that segment.
    ///
    /// The method scans every file known to the server to find:
    /// - `namespace` declarations that start with the old prefix
    /// - `use` statements that reference the old prefix
    /// - Inline FQN references (in code and docblocks)
    ///
    /// It also emits `RenameFile` operations when a PSR-4 mapping
    /// exists so that the directory structure stays consistent.
    pub(super) fn build_namespace_rename_edit(
        &self,
        full_ns: &str,
        segment_idx: usize,
        new_segment: &str,
    ) -> Option<WorkspaceEdit> {
        let segments: Vec<&str> = full_ns.split('\\').collect();
        if segment_idx >= segments.len() {
            return None;
        }

        // Build the old prefix up to and including the renamed segment.
        // For example, if `full_ns` is `App\Bar\Service` and we rename
        // segment 1 (`Bar`), `old_prefix` is `App\Bar`.
        let old_prefix: String = segments[..=segment_idx].join("\\");
        let mut new_segments = segments.clone();
        new_segments[segment_idx] = new_segment;
        let new_prefix: String = new_segments[..=segment_idx].join("\\");

        self.build_namespace_prefix_rename_edit(&old_prefix, &new_prefix)
    }

    pub(super) fn build_namespace_prefix_rename_edit(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Option<WorkspaceEdit> {
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        // Scan all known files. The live per-file maps only contain files
        // that have been fully parsed through update_ast, but workspace
        // scanning can index many more class files via uri_classes_index.
        // Include those URIs too so namespace rename updates references in
        // unopened workspace files.
        let all_uris: Vec<String> = {
            let nmap = self.file_namespaces.read();
            let umap = self.file_imports.read();
            let smap = self.symbol_maps.read();
            let cmap = self.uri_classes_index.read();
            let ofiles = self.open_files.read();
            let workspace_root = self.workspace_root.read().clone();
            let vendor_dir_paths = self.vendor_dir_paths.lock().clone();
            let mut uris: std::collections::HashSet<String> = std::collections::HashSet::new();
            for uri in nmap.keys() {
                uris.insert(uri.clone());
            }
            for uri in umap.keys() {
                uris.insert(uri.clone());
            }
            for uri in smap.keys() {
                uris.insert(uri.clone());
            }
            for uri in cmap.keys() {
                uris.insert(uri.clone());
            }
            for uri in ofiles.keys() {
                uris.insert(uri.clone());
            }

            if let Some(root) = workspace_root {
                for path in crate::references::collect_php_files_gitignore(&root, &vendor_dir_paths)
                {
                    if let Ok(uri) = Url::from_file_path(&path) {
                        uris.insert(uri.to_string());
                    }
                }
            }

            uris.into_iter().collect()
        };

        // Skip vendor files.
        let vendor_prefixes = self.vendor_uri_prefixes.lock().clone();

        for file_uri in &all_uris {
            if vendor_prefixes
                .iter()
                .any(|p| file_uri.starts_with(p.as_str()))
            {
                continue;
            }

            let content = match self.get_file_content(file_uri) {
                Some(c) => c,
                None => continue,
            };

            let parsed_uri = match Url::parse(file_uri) {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!(
                        "rename: dropping edits for file with unparseable URI {file_uri:?}: {e}"
                    );
                    continue;
                }
            };

            let mut file_edits: Vec<TextEdit> = Vec::new();

            // 1. Update `namespace` declarations.
            //    Find lines like `namespace App\Bar\Service;` or
            //    `namespace App\Bar\Service {` where the namespace
            //    starts with `old_prefix`.
            self.collect_namespace_decl_edits(&content, old_prefix, new_prefix, &mut file_edits);

            // 2. Update `use` statements.
            self.collect_use_statement_edits(&content, old_prefix, new_prefix, &mut file_edits);

            // 3. Update inline FQN references from the symbol map.
            self.collect_fqn_reference_edits(
                file_uri,
                &content,
                old_prefix,
                new_prefix,
                &mut file_edits,
            );

            if !file_edits.is_empty() {
                // Sort edits by start position descending so they don't
                // interfere with each other when applied.
                file_edits.sort_by(|a, b| {
                    b.range
                        .start
                        .line
                        .cmp(&a.range.start.line)
                        .then(b.range.start.character.cmp(&a.range.start.character))
                });
                // Deduplicate overlapping edits (keep first = largest line).
                file_edits.dedup_by(|a, b| ranges_overlap(&a.range, &b.range));
                changes.entry(parsed_uri).or_default().extend(file_edits);
            }
        }

        if changes.is_empty() {
            return None;
        }

        // PSR-4 directory rename: if a mapping exists, emit RenameFile
        // operations to move the directory.
        if let Some(ops) = self.build_namespace_psr4_rename_ops(old_prefix, new_prefix)
            && !ops.is_empty()
            && self.supports_file_rename.load(Ordering::Acquire)
        {
            let mut doc_ops: Vec<DocumentChangeOperation> = Vec::new();

            // Add directory/file rename operations first.
            for (old_uri, new_uri) in &ops {
                doc_ops.push(DocumentChangeOperation::Op(ResourceOp::Rename(
                    RenameFile {
                        old_uri: old_uri.clone(),
                        new_uri: new_uri.clone(),
                        options: None,
                        annotation_id: None,
                    },
                )));
            }

            // Convert text edits to document changes. Rewrite URIs
            // that fall inside a renamed directory.
            for (uri, edits) in changes {
                let target_uri = ops
                    .iter()
                    .find_map(|(old_u, new_u)| {
                        let old_str = old_u.as_str();
                        let uri_str = uri.as_str();
                        if let Some(rest) = uri_str.strip_prefix(old_str) {
                            Url::parse(&format!("{}{}", new_u.as_str(), rest)).ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(uri);

                let text_doc_edit = TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: target_uri,
                        version: None,
                    },
                    edits: edits.into_iter().map(OneOf::Left).collect(),
                };
                doc_ops.push(DocumentChangeOperation::Edit(text_doc_edit));
            }

            return Some(WorkspaceEdit {
                changes: None,
                document_changes: Some(DocumentChanges::Operations(doc_ops)),
                change_annotations: None,
            });
        }

        Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    /// Collect text edits for `namespace` declaration lines where the
    /// namespace starts with `old_prefix`.
    fn collect_namespace_decl_edits(
        &self,
        content: &str,
        old_prefix: &str,
        new_prefix: &str,
        edits: &mut Vec<TextEdit>,
    ) {
        let old_prefix_lower = old_prefix.to_lowercase();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("namespace ") else {
                continue;
            };
            let rest = rest.trim();
            // Strip trailing `;` or `{`.
            let ns_name = rest.trim_end_matches(';').trim_end_matches('{').trim();

            if ns_name.is_empty() {
                continue;
            }

            let ns_lower = ns_name.to_lowercase();
            // The namespace must equal old_prefix or start with old_prefix + `\`.
            if ns_lower != old_prefix_lower
                && !ns_lower.starts_with(&format!("{}\\", old_prefix_lower))
            {
                continue;
            }

            // Build the new namespace name by replacing the prefix.
            let new_ns = if ns_name.len() == old_prefix.len() {
                new_prefix.to_string()
            } else {
                format!("{}{}", new_prefix, &ns_name[old_prefix.len()..])
            };

            // Find the byte range of the namespace name within the line.
            let line_start_byte = line_start_byte_offset(content, line_idx);
            let ns_offset_in_line = line.find(ns_name).unwrap_or(0);
            let ns_start = line_start_byte + ns_offset_in_line;
            let ns_end = ns_start + ns_name.len();

            edits.push(TextEdit {
                range: Range {
                    start: offset_to_position(content, ns_start),
                    end: offset_to_position(content, ns_end),
                },
                new_text: new_ns,
            });
        }
    }

    /// Collect text edits for `use` statement lines that reference the
    /// old namespace prefix.
    fn collect_use_statement_edits(
        &self,
        content: &str,
        old_prefix: &str,
        new_prefix: &str,
        edits: &mut Vec<TextEdit>,
    ) {
        let old_prefix_lower = old_prefix.to_lowercase();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("use ") else {
                continue;
            };
            let rest = rest.trim();
            // Handle `use function` and `use const` prefixes.
            let rest = rest
                .strip_prefix("function ")
                .or_else(|| rest.strip_prefix("const "))
                .unwrap_or(rest)
                .trim();

            let rest = rest.strip_suffix(';').unwrap_or(rest).trim();

            // Handle group use: `use App\Old\{Foo, Bar};`
            if let Some(brace_pos) = rest.find('{') {
                let group_prefix = rest[..brace_pos].trim_end_matches('\\').trim();
                let group_lower = group_prefix.to_lowercase();

                if group_lower == old_prefix_lower
                    || group_lower.starts_with(&format!("{}\\", old_prefix_lower))
                {
                    let new_group_prefix = if group_prefix.len() == old_prefix.len() {
                        new_prefix.to_string()
                    } else {
                        format!("{}{}", new_prefix, &group_prefix[old_prefix.len()..])
                    };

                    let line_start_byte = line_start_byte_offset(content, line_idx);
                    let prefix_offset_in_line = line.find(group_prefix).unwrap_or(0);
                    let prefix_start = line_start_byte + prefix_offset_in_line;
                    let prefix_end = prefix_start + group_prefix.len();

                    edits.push(TextEdit {
                        range: Range {
                            start: offset_to_position(content, prefix_start),
                            end: offset_to_position(content, prefix_end),
                        },
                        new_text: new_group_prefix,
                    });
                }
                continue;
            }

            // Simple use: `use App\Old\Foo;` or `use App\Old\Foo as Bar;`
            let (fqn_part, _alias_part) = if let Some(as_pos) = rest.find(" as ") {
                (rest[..as_pos].trim(), Some(&rest[as_pos + 4..]))
            } else {
                (rest, None)
            };

            let fqn_lower = fqn_part.to_lowercase();
            if fqn_lower == old_prefix_lower
                || fqn_lower.starts_with(&format!("{}\\", old_prefix_lower))
            {
                let new_fqn = if fqn_part.len() == old_prefix.len() {
                    new_prefix.to_string()
                } else {
                    format!("{}{}", new_prefix, &fqn_part[old_prefix.len()..])
                };

                let line_start_byte = line_start_byte_offset(content, line_idx);
                let fqn_offset_in_line = line.find(fqn_part).unwrap_or(0);
                let fqn_start = line_start_byte + fqn_offset_in_line;
                let fqn_end = fqn_start + fqn_part.len();

                edits.push(TextEdit {
                    range: Range {
                        start: offset_to_position(content, fqn_start),
                        end: offset_to_position(content, fqn_end),
                    },
                    new_text: new_fqn,
                });
            }
        }
    }

    /// Collect text edits for inline FQN references (e.g. `\App\Old\Foo`
    /// in type hints or docblocks) that contain the old prefix.
    fn collect_fqn_reference_edits(
        &self,
        file_uri: &str,
        content: &str,
        old_prefix: &str,
        new_prefix: &str,
        edits: &mut Vec<TextEdit>,
    ) {
        let symbol_map = match self.symbol_maps.read().get(file_uri) {
            Some(sm) => sm.clone(),
            None => return,
        };

        let old_prefix_lower = old_prefix.to_lowercase();

        for span in &symbol_map.spans {
            let name = match &span.kind {
                SymbolKind::ClassReference {
                    name, is_fqn: true, ..
                } => name,
                _ => continue,
            };

            // Only process references that contain a backslash (FQN-style).
            let name_normalized = strip_fqn_prefix(name);
            let name_lower = name_normalized.to_lowercase();

            if name_lower != old_prefix_lower
                && !name_lower.starts_with(&format!("{}\\", old_prefix_lower))
            {
                continue;
            }

            // Check source text to see if this is an inline FQN reference
            // (contains `\` in source).  Use-statement references are
            // handled separately by collect_use_statement_edits.
            let source = content
                .get(span.start as usize..span.end as usize)
                .unwrap_or("");

            // Skip use-statement references (they don't have `\` in span
            // unless they are inline FQN like `\App\Foo` in code).
            // Actually, use-statement spans DO contain the full FQN.
            // We rely on deduplication to handle overlaps.

            let new_name = if name_normalized.len() == old_prefix.len() {
                if name.starts_with('\\') {
                    format!("\\{}", new_prefix)
                } else {
                    new_prefix.to_string()
                }
            } else {
                let suffix = &name_normalized[old_prefix.len()..];
                if name.starts_with('\\') {
                    format!("\\{}{}", new_prefix, suffix)
                } else {
                    format!("{}{}", new_prefix, suffix)
                }
            };

            // Only emit an edit if the text actually changes.
            if source == new_name {
                continue;
            }

            edits.push(TextEdit {
                range: Range {
                    start: offset_to_position(content, span.start as usize),
                    end: offset_to_position(content, span.end as usize),
                },
                new_text: new_name,
            });
        }
    }

    /// Determine PSR-4 directory rename operations for a namespace rename.
    ///
    /// Returns pairs of `(old_uri, new_uri)` for directories that should
    /// be renamed, or `None` if no PSR-4 mapping applies.
    fn build_namespace_psr4_rename_ops(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Option<Vec<(Url, Url)>> {
        let psr4 = self.psr4_mappings.read();
        let workspace_root = self.workspace_root.read().clone()?;

        let mut ops: Vec<(Url, Url)> = Vec::new();

        for mapping in psr4.iter() {
            let mapping_ns = mapping.prefix.trim_end_matches('\\');

            // Check if old_prefix starts with this PSR-4 mapping's namespace.
            let old_lower = old_prefix.to_lowercase();
            let mapping_lower = mapping_ns.to_lowercase();

            let relative_ns = if old_lower == mapping_lower {
                ""
            } else if old_lower.starts_with(&format!("{}\\", mapping_lower)) {
                &old_prefix[mapping_ns.len() + 1..]
            } else {
                continue;
            };

            let new_relative_ns = if old_prefix.len() == mapping_ns.len() {
                // We're renaming at the PSR-4 root itself — new_prefix
                // replaces the mapping prefix entirely in the path.
                let new_without_mapping = &new_prefix[mapping_ns.len()..];
                new_without_mapping.trim_start_matches('\\').to_string()
            } else {
                let suffix = &new_prefix[mapping_ns.len() + 1..];
                suffix.to_string()
            };

            // Build old and new directory paths.
            let base_dir = workspace_root.join(&mapping.base_path);
            let old_dir = if relative_ns.is_empty() {
                base_dir.clone()
            } else {
                base_dir.join(relative_ns.replace('\\', std::path::MAIN_SEPARATOR_STR))
            };

            let new_dir = if new_relative_ns.is_empty() {
                base_dir
            } else {
                base_dir.join(new_relative_ns.replace('\\', std::path::MAIN_SEPARATOR_STR))
            };

            if old_dir == new_dir {
                continue;
            }

            // Only emit if the old directory actually exists.
            if !old_dir.is_dir() {
                continue;
            }

            let old_url = Url::from_file_path(&old_dir).ok()?;
            let new_url = Url::from_file_path(&new_dir).ok()?;
            ops.push((old_url, new_url));
        }

        if ops.is_empty() { None } else { Some(ops) }
    }
}

// ─── Namespace segment helpers ──────────────────────────────────────────────

/// Given a namespace name (e.g. `"App\\Bar\\Service"`) and its starting
/// byte offset in the source, find which segment the cursor (byte
/// offset) falls on.
///
/// Returns `(segment_text, segment_start_offset, segment_end_offset)`.
pub(super) fn find_namespace_segment_at_offset(
    ns_name: &str,
    ns_start: u32,
    cursor: u32,
) -> Option<(&str, u32, u32)> {
    let mut offset = ns_start;
    for segment in ns_name.split('\\') {
        let seg_end = offset + segment.len() as u32;
        if cursor >= offset && cursor < seg_end {
            return Some((segment, offset, seg_end));
        }
        // Skip past the segment and the `\` separator.
        offset = seg_end + 1;
    }
    // If cursor is exactly at the end of the last segment, return that.
    let last_seg = ns_name.rsplit('\\').next()?;
    let last_start = ns_start + ns_name.len() as u32 - last_seg.len() as u32;
    let last_end = ns_start + ns_name.len() as u32;
    if cursor == last_end {
        return Some((last_seg, last_start, last_end));
    }
    None
}
