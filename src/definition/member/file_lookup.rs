//! File loading and member position lookup helpers.
//!
//! These functions locate the source file for a class and find the exact
//! position of a member declaration within it.  They are used by
//! `resolve_member_definition_with` after the declaring class has been
//! identified by the `declaring` module.

use std::sync::Arc;

use tower_lsp::lsp_types::Position;

use crate::Backend;
use crate::types::ClassInfo;
use crate::util::short_name;

use super::MemberKind;

impl Backend {
    /// Reload the raw (unmerged) `ClassInfo` for a candidate.
    ///
    /// Candidates returned by `resolve_target_classes` may be
    /// fully-resolved classes with virtual/mixin members baked into
    /// their `methods` list (this happens when `type_hint_to_classes_typed`
    /// calls `resolve_class_fully` to apply generic substitutions).
    /// `find_declaring_class` needs the raw class so it can trace
    /// member declarations through the real inheritance and mixin
    /// chain instead of short-circuiting on a merged method.
    ///
    /// Returns `Some(raw)` when a reload succeeds, or `None` when the
    /// class cannot be reloaded (e.g. synthetic/anonymous classes).
    pub(in crate::definition) fn reload_raw_class(
        candidate: &ClassInfo,
        all_classes: &[Arc<ClassInfo>],
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Option<ClassInfo> {
        let fqn = candidate.fqn();
        crate::class_lookup::find_class_by_name(all_classes, &fqn)
            .map(|arc| ClassInfo::clone(arc))
            .or_else(|| class_loader(&fqn).map(Arc::unwrap_or_clone))
    }

    /// Find the file URI and content for the file that contains a given class.
    ///
    /// `class_name` can be a short name (e.g. `"Kernel"`) or a
    /// fully-qualified name (e.g. `"Illuminate\\Foundation\\Console\\Kernel"`).
    /// When a namespace prefix is present the file's namespace (from
    /// `namespace_map`) must match for the class to be returned.  This
    /// prevents short-name collisions when a child class and its parent
    /// share the same simple name but live in different namespaces.
    ///
    /// Searches the `uri_classes_index` (which includes files loaded via PSR-4 by
    /// `find_or_load_class`) and returns `(uri, content)`.
    pub(crate) fn find_class_file_content(
        &self,
        class_name: &str,
        current_uri: &str,
        current_content: &str,
    ) -> Option<(String, String)> {
        let uri = self.find_class_file_uri(class_name, current_uri)?;

        // Get the file content.
        let file_content = if uri == current_uri {
            current_content.to_string()
        } else {
            self.get_file_content(&uri)?
        };

        Some((uri, file_content))
    }

    /// The URI of the file that declares `class_name`, without reading it.
    ///
    /// The lookup half of [`find_class_file_content`](Self::find_class_file_content),
    /// for callers that already hold the content or want it as a shared
    /// `Arc<String>` rather than a copy.
    pub(crate) fn find_class_file_uri(
        &self,
        class_name: &str,
        current_uri: &str,
    ) -> Option<String> {
        let last_segment = short_name(class_name);
        let expected_ns: Option<&str> = if class_name.contains('\\') {
            Some(&class_name[..class_name.len() - last_segment.len() - 1])
        } else {
            None
        };

        // Search the uri_classes_index for the file containing this class.
        {
            let map = self.symbols.uri_classes_index.read();
            let nmap = self.file_namespaces.read();

            // Check whether a class with the right short name and
            // namespace lives in this file.  Uses the per-class
            // `file_namespace` field first (correct for multi-namespace
            // files like example.php), falling back to the file-level
            // `namespace_map` for single-namespace files.
            let class_in_file = |file_uri: &str, classes: &[Arc<ClassInfo>]| -> bool {
                match expected_ns {
                    None => classes.iter().any(|c| c.name == last_segment),
                    Some(exp) => {
                        // Prefer per-class file_namespace (handles
                        // multi-namespace files correctly).
                        let found_via_class_ns = classes.iter().any(|c| {
                            c.name == last_segment && c.file_namespace.as_deref() == Some(exp)
                        });
                        if found_via_class_ns {
                            return true;
                        }
                        // Fall back to file-level namespace_map for
                        // classes that don't have file_namespace set
                        // (e.g. single-namespace files, stubs).
                        let file_ns = nmap
                            .get(file_uri)
                            .and_then(|spans| spans.first().and_then(|s| s.namespace.as_deref()));
                        file_ns == Some(exp) && classes.iter().any(|c| c.name == last_segment)
                    }
                }
            };

            // Check the current file first (common case: $this->method).
            if let Some(classes) = map.get(current_uri) {
                if class_in_file(current_uri, classes) {
                    Some(current_uri.to_string())
                } else {
                    // Search other files.
                    map.iter()
                        .find(|(u, classes)| class_in_file(u, classes))
                        .map(|(u, _)| u.clone())
                }
            } else {
                map.iter()
                    .find(|(u, classes)| class_in_file(u, classes))
                    .map(|(u, _)| u.clone())
            }
        }
        .or_else(|| {
            // Fallback: the target file may have been closed (didClose
            // clears uri_classes_index) or was never opened.  Check fqn_uri_index
            // which survives close.
            self.symbols.fqn_uri_index.read().get(class_name).cloned()
        })
        .or_else(|| {
            // Last resort: resolve the class via PSR-4 mappings to get
            // the file URI.  This handles classes that were loaded into
            // fqn_index (via parse_and_cache_file) but whose uri_classes_index
            // entry was later cleared by didClose, and whose fqn_uri_index
            // entry was never created (parse_and_cache_content does not
            // populate fqn_uri_index).
            let workspace_root = self.workspace.workspace_root.read().clone()?;
            let mappings = self.workspace.psr4_mappings.read();
            let file_path =
                crate::composer::resolve_class_path(&mappings, &workspace_root, class_name)?;
            Some(crate::util::path_to_uri(&file_path))
        })
    }

    /// Find the position of `class`'s member declaration (method,
    /// property, or constant) in `content`, the source of the file `class`
    /// is declared in.
    ///
    /// A declared member carries its name's offset; a virtual `@method` /
    /// `@property` member is found in the tag of `class`'s own docblock that
    /// declares it.
    pub(crate) fn find_member_position(
        content: &str,
        member_name: &str,
        kind: MemberKind,
        class: &ClassInfo,
    ) -> Option<Position> {
        let offset = class
            .member_name_offset(member_name, kind.as_str())
            .or_else(|| docblock_tag_offset(class, member_name, kind))?;
        if offset as usize > content.len() {
            return None;
        }
        let mut pos = crate::text_position::offset_to_position(content, offset as usize);
        // For properties, place the cursor on the first letter after `$` so
        // that a second go-to-definition triggers type-hint resolution.
        if kind == MemberKind::Property {
            pos.character += 1;
        }
        Some(pos)
    }
}

/// The offset in its file of the `@property` (at the `$`) or `@method` (at
/// the name) tag in `class`'s own docblock that declares `member_name`.
fn docblock_tag_offset(class: &ClassInfo, member_name: &str, kind: MemberKind) -> Option<u32> {
    let docblock = class.class_docblock.as_deref()?;
    let is_word_boundary = |c: u8| !c.is_ascii_alphanumeric() && c != b'_';
    let mut line_start = 0;
    for line in docblock.split_inclusive('\n') {
        let tag = line
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches('*')
            .trim();
        let found = match kind {
            MemberKind::Property if tag.starts_with("@property") => {
                let needle = format!("${member_name}");
                line.match_indices(&needle)
                    .map(|(col, _)| col)
                    .find(|&col| {
                        line.as_bytes()
                            .get(col + needle.len())
                            .is_none_or(|&c| is_word_boundary(c))
                    })
            }
            // The name is followed by `(`; a return type spelled like the
            // method (`@method string string()`) is not.
            MemberKind::Method
                if tag.starts_with("@method") || tag.starts_with("@psalm-method") =>
            {
                line.match_indices(member_name)
                    .map(|(col, _)| col)
                    .find(|&col| {
                        line.as_bytes().get(col + member_name.len()) == Some(&b'(')
                            && (col == 0 || is_word_boundary(line.as_bytes()[col - 1]))
                    })
            }
            _ => None,
        };
        if let Some(col) = found {
            return Some(class.class_docblock_start + (line_start + col) as u32);
        }
        line_start += line.len();
    }
    None
}
