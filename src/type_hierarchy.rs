//! Type hierarchy support (`textDocument/prepareTypeHierarchy`,
//! `typeHierarchy/supertypes`, `typeHierarchy/subtypes`).
//!
//! Shows the class hierarchy (supertypes and subtypes) for a
//! class/interface/trait/enum under the cursor. Uses the same
//! infrastructure as go-to-implementation for subtypes (the
//! `find_implementors` scan) and the already-resolved inheritance
//! chain for supertypes.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::{SelfStaticParentKind, SymbolKind as MapSymbolKind};
use crate::types::{ClassInfo, ClassLikeKind};
use crate::util::{find_class_at_offset, offset_to_position, position_to_offset, short_name};

impl Backend {
    /// Prepare the type hierarchy for the symbol under the cursor.
    ///
    /// Finds the class/interface/trait/enum at `position` and returns a
    /// single-element `Vec<TypeHierarchyItem>` so the client can then
    /// ask for supertypes or subtypes.
    pub(crate) fn prepare_type_hierarchy_impl(
        &self,
        uri: &str,
        content: &str,
        position: Position,
    ) -> Option<Vec<TypeHierarchyItem>> {
        let offset = position_to_offset(content, position);
        let span = self.lookup_symbol_map(uri, offset)?;

        let fqn = match &span.kind {
            MapSymbolKind::ClassReference { name, is_fqn, .. } => {
                if *is_fqn {
                    name.trim_start_matches('\\').to_string()
                } else {
                    let ctx = self.file_context(uri);
                    ctx.resolve_name_at(name, span.start)
                }
            }
            MapSymbolKind::ClassDeclaration { name } => {
                let ctx = self.file_context(uri);
                ctx.resolve_name_at(name, span.start)
            }
            MapSymbolKind::SelfStaticParent(ssp_kind) => {
                let classes: Vec<std::sync::Arc<ClassInfo>> = self
                    .uri_classes_index
                    .read()
                    .get(uri)
                    .cloned()
                    .unwrap_or_default();
                let current_class = find_class_at_offset(&classes, offset)?;

                if *ssp_kind == SelfStaticParentKind::Parent {
                    let parent_name = current_class.parent_class.as_ref()?;
                    let ctx = self.file_context(uri);
                    ctx.resolve_name_at(parent_name, span.start)
                } else {
                    // self, static, or $this
                    crate::util::build_fqn(
                        &current_class.name,
                        current_class.file_namespace.as_deref(),
                    )
                }
            }
            _ => return None,
        };

        let class_info = self.find_or_load_class(&fqn)?;

        let item = self.build_hierarchy_item_for_class(&class_info, &fqn);
        Some(vec![item])
    }

    /// Return the supertypes (parent class + implemented interfaces) of
    /// a type hierarchy item.
    pub(crate) fn supertypes_impl(
        &self,
        item: &TypeHierarchyItem,
    ) -> Option<Vec<TypeHierarchyItem>> {
        let fqn = extract_fqn_from_data(item)?;
        let class_info = self.find_or_load_class(&fqn)?;

        let item_uri = item.uri.as_str();

        let mut result = Vec::new();

        // Collect all supertype names: parent class first, then interfaces.
        let mut supertype_names: Vec<&str> = Vec::new();
        if let Some(ref parent) = class_info.parent_class {
            supertype_names.push(parent);
        }
        for iface in &class_info.interfaces {
            supertype_names.push(iface);
        }

        // For traits, also include used traits as supertypes since
        // traits can `use` other traits.
        if class_info.kind == ClassLikeKind::Trait {
            for tr in &class_info.used_traits {
                supertype_names.push(tr);
            }
        }

        // Resolve each supertype to a ClassInfo and build a hierarchy item.
        // Parent/interface/trait names in ClassInfo are already resolved to
        // FQN during post-processing, so try loading them directly first.
        // Fall back to resolve_to_fqn (via the item's file context) for
        // names that were not post-processed (e.g. stubs, edge cases).
        let ctx = self.file_context(item_uri);

        for name in supertype_names {
            let (resolved_fqn, super_info) = if let Some(info) = self.find_or_load_class(name) {
                (name.to_string(), info)
            } else {
                let fqn = Self::resolve_to_fqn(name, &ctx.use_map, &ctx.namespace);
                match self.find_or_load_class(&fqn) {
                    Some(info) => (fqn, info),
                    None => continue,
                }
            };

            let super_item = self.build_hierarchy_item_for_class(&super_info, &resolved_fqn);
            result.push(super_item);
        }

        Some(result)
    }

    /// Return the subtypes (classes that extend/implement) of a type
    /// hierarchy item.
    pub(crate) fn subtypes_impl(&self, item: &TypeHierarchyItem) -> Option<Vec<TypeHierarchyItem>> {
        let fqn = extract_fqn_from_data(item)?;
        let short = short_name(&fqn);

        let item_uri = item.uri.as_str();
        let ctx = self.file_context(item_uri);
        let class_loader = self.class_loader(&ctx);

        // direct_only = true so only immediate children are returned;
        // the client walks the tree one level at a time.
        //
        // We can't shortcut to the reverse inheritance index (`gti_index`)
        // alone: it is populated only from files that have been fully
        // parsed, so a parent may have one child already indexed while
        // other children live in files discovered on disk but not yet
        // parsed.  `find_implementors` uses `gti_index` as its first phase
        // and then scans for the rest, so it returns the complete set.
        let implementors = self.find_implementors(short, &fqn, &class_loader, true, true, false);

        let mut result = Vec::new();
        for imp in &implementors {
            let imp_fqn = crate::util::build_fqn(&imp.name, imp.file_namespace.as_deref());
            let imp_item = self.build_hierarchy_item_for_class(imp, &imp_fqn);
            result.push(imp_item);
        }

        Some(result)
    }

    /// Build a `TypeHierarchyItem` for a class, looking up its file
    /// content from `open_files` / `uri_classes_index` / disk so that byte
    /// offsets can be converted to LSP positions correctly.
    fn build_hierarchy_item_for_class(
        &self,
        class_info: &ClassInfo,
        fqn: &str,
    ) -> TypeHierarchyItem {
        // Locate the file that contains this class.  We try
        // find_class_file_content first with a dummy current URI so
        // it searches all files in the uri_classes_index.  If that fails, fall
        // back to get_file_content for files that might be open or on
        // disk.
        let (class_uri, class_content) = self
            .find_class_file_content(fqn, "", "")
            .or_else(|| self.find_class_file_content(&class_info.name, "", ""))
            .unwrap_or_else(|| {
                // Last resort: try to find the URI from the fqn_uri_index
                // and read from disk / open_files.
                let uri = self
                    .fqn_uri_index
                    .read()
                    .get(fqn)
                    .cloned()
                    .unwrap_or_default();
                let content = if !uri.is_empty() {
                    self.get_file_content(&uri).unwrap_or_default()
                } else {
                    String::new()
                };
                (uri, content)
            });

        build_type_hierarchy_item(class_info, fqn, &class_uri, &class_content)
    }
}

/// Build a `TypeHierarchyItem` from a resolved `ClassInfo`.
///
/// The `fqn` is stored in the `data` field so that subsequent
/// supertype/subtype requests can resolve efficiently without
/// re-parsing.
fn build_type_hierarchy_item(
    class_info: &ClassInfo,
    fqn: &str,
    uri: &str,
    content: &str,
) -> TypeHierarchyItem {
    let parsed_uri = Url::parse(uri).unwrap_or_else(|_| Url::parse("file:///unknown").unwrap());

    let kind = class_like_kind_to_symbol_kind(class_info.kind);

    // Compute the selection range (the class name token).
    // keyword_offset points to the `class`/`interface`/`trait`/`enum`
    // keyword.  The class name follows after a space, so we need to
    // find it.  A simple heuristic: scan forward from keyword_offset
    // to find the start of the class name in the source.
    let has_content = !content.is_empty();
    let content_bytes = content.as_bytes();

    let (selection_range, range) = if has_content && class_info.keyword_offset > 0 {
        // Find the class name in source starting from the keyword.
        // The keyword is "class", "interface", "trait", or "enum".
        // The name follows after whitespace.
        let kw_off = class_info.keyword_offset as usize;
        let name_start = find_name_start(content_bytes, kw_off);
        let name_end = name_start + class_info.name.len();

        // Clamp to content length to avoid panics.
        let name_start = name_start.min(content.len());
        let name_end = name_end.min(content.len());

        let sel_start = offset_to_position(content, name_start);
        let sel_end = offset_to_position(content, name_end);
        let sel_range = Range::new(sel_start, sel_end);

        // Full range from start_offset to end_offset.
        let full_range = if class_info.end_offset > class_info.start_offset {
            let start = (class_info.start_offset as usize).min(content.len());
            let end = (class_info.end_offset as usize).min(content.len());
            Range::new(
                offset_to_position(content, start),
                offset_to_position(content, end),
            )
        } else {
            sel_range
        };

        (sel_range, full_range)
    } else if has_content && class_info.keyword_offset == 0 && class_info.start_offset > 0 {
        // keyword_offset is 0 (unknown) but we have body offsets.
        let start = (class_info.start_offset as usize).min(content.len());
        let end = (class_info.end_offset as usize).min(content.len());
        let r = Range::new(
            offset_to_position(content, start),
            offset_to_position(content, end),
        );
        (r, r)
    } else {
        // No content or no offsets — use 0,0.
        let zero = Range::new(Position::new(0, 0), Position::new(0, 0));
        (zero, zero)
    };

    let tags = if class_info.deprecation_message.is_some() {
        Some(SymbolTag::DEPRECATED)
    } else {
        None
    };

    let detail = namespace_detail(fqn);

    TypeHierarchyItem {
        name: class_info.name.to_string(),
        kind,
        tags,
        detail,
        uri: parsed_uri,
        range,
        selection_range,
        data: Some(serde_json::json!({"fqn": fqn})),
    }
}

/// Starting from `keyword_offset` (which points to the keyword like
/// `class`), skip the keyword text and whitespace to find where the
/// class name starts.
fn find_name_start(content: &[u8], keyword_offset: usize) -> usize {
    let mut pos = keyword_offset;
    let len = content.len();

    // Skip the keyword (letters).
    while pos < len && content[pos].is_ascii_alphabetic() {
        pos += 1;
    }

    // Skip all whitespace (including newlines) between keyword and name,
    // so the legal `class\nFoo {` layout locates the name correctly.
    while pos < len && content[pos].is_ascii_whitespace() {
        pos += 1;
    }

    pos
}

/// Map a `ClassLikeKind` to the corresponding LSP `SymbolKind`.
///
/// Traits map to `STRUCT` because LSP does not have a dedicated trait
/// symbol kind.
fn class_like_kind_to_symbol_kind(kind: ClassLikeKind) -> tower_lsp::lsp_types::SymbolKind {
    match kind {
        ClassLikeKind::Class => tower_lsp::lsp_types::SymbolKind::CLASS,
        ClassLikeKind::Interface => tower_lsp::lsp_types::SymbolKind::INTERFACE,
        ClassLikeKind::Trait => tower_lsp::lsp_types::SymbolKind::STRUCT,
        ClassLikeKind::Enum => tower_lsp::lsp_types::SymbolKind::ENUM,
    }
}

/// Extract the namespace portion of a fully-qualified name.
///
/// Returns `Some("App\\Models")` for `"App\\Models\\User"`, or `None`
/// for unqualified names that have no namespace separator.
fn namespace_detail(fqn: &str) -> Option<String> {
    let idx = fqn.rfind('\\')?;
    let ns = &fqn[..idx];
    if ns.is_empty() {
        None
    } else {
        Some(ns.to_string())
    }
}

/// Extract the FQN string from the `data` field of a `TypeHierarchyItem`.
fn extract_fqn_from_data(item: &TypeHierarchyItem) -> Option<String> {
    let data = item.data.as_ref()?;
    data.get("fqn")?.as_str().map(|s| s.to_string())
}
