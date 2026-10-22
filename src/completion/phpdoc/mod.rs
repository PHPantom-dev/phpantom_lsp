//! PHPDoc tag completion.
//!
//! ## Smart completion context
//!
//! The [`SmartContext`] struct bundles optional enrichment data that the
//! handler passes into [`build_phpdoc_completions`]:
//!
//! - An **inferred inline variable type** (for pre-filling `@var` above
//!   variable assignments).
//! - A **class loader** callback (for enriching `@param`, `@return`, and
//!   `@var` type hints with `@template` parameters).
//!
//! When the user types `@` inside a `/** … */` docblock, this module
//! provides context-aware tag suggestions.  The tags offered depend on
//! what PHP symbol follows the docblock:
//!
//! - **Function / Method**: `@param`, `@return`, `@throws`, …
//! - **Class / Interface / Trait / Enum**: `@property`, `@method`, `@mixin`, `@template`, …
//! - **Property**: `@var`, `@deprecated`, …
//! - **Constant**: `@var`, `@deprecated`, …
//! - **Unknown / file-level**: all tags
//!
//! When the symbol following the docblock can be parsed, completions are
//! pre-filled with concrete type and parameter information extracted from
//! the declaration (e.g. `@param string $name` instead of bare `@param`).
//!
//! Smart `@throws` completion analyses the function/method body for
//! `throw new ExceptionType(…)` statements that are not caught by a
//! `try/catch` block enclosing them, and suggests documenting each
//! uncaught exception type.  When the exception class is not yet imported
//! via a `use` statement, an `additional_text_edits` entry is added to
//! insert the import automatically.
//!
//! Context detection and symbol info extraction live in the sibling
//! `context` module and are re-exported here for backward compatibility.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::php_type::PhpType;
use crate::types::ClassInfo;

/// Callback signature for resolving a class name to its [`ClassInfo`].
///
/// Used by [`SmartContext`] and other enrichment code to look up
/// `@template` parameters and other class metadata.
pub type ClassLoaderFn<'a> = &'a crate::class_lookup::SourceClassLoader<'a>;

/// Callback that resolves a function name to its [`FunctionInfo`].
///
/// Used by [`SmartContext`] for cross-file `@throws` propagation from
/// standalone function calls.
pub type FunctionLoaderFn<'a> = &'a dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>;

// Re-export comment-position helpers so existing consumers (tests,
// handler, catch_completion) that import from `phpdoc::` keep working.
pub(crate) use super::source::comment_position::position_to_byte_offset;
pub use super::source::comment_position::{is_inside_docblock, is_inside_non_doc_comment};

// Re-export all public items from `context` so that existing
// consumers (`handler.rs`, tests) that import from `phpdoc::` keep
// working without path changes.
mod context;
pub(crate) mod generation;
mod helpers;
mod smart_tags;
mod tags;

pub use context::{
    DocblockContext, DocblockTypingContext, SymbolInfo, detect_context,
    detect_docblock_typing_position, extract_phpdoc_prefix, extract_symbol_info,
};
use smart_tags::{TagSite, push_smart_tag_items};
use tags::{generic_tag_item, tags_for_context};

// ─── Existing-tag scanning ──────────────────────────────────────────────────

/// Collect the names of parameters already documented with `@param` tags
/// in the current docblock above the cursor.
pub fn find_existing_param_tags(content: &str, position: Position) -> Vec<String> {
    use crate::docblock::TagKind;

    let byte_offset = position_to_byte_offset(content, position);
    let before_cursor = &content[..byte_offset.min(content.len())];

    // Find the opening `/**`
    let open_pos = match before_cursor.rfind("/**") {
        Some(pos) => pos,
        None => return Vec::new(),
    };

    let docblock_so_far = &before_cursor[open_pos..];

    let info = match crate::docblock::parser::parse_docblock_for_tags(docblock_so_far) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let mut existing = Vec::new();
    for tag in info.tags_by_kind(TagKind::Param) {
        let rest = tag.description.trim();
        // @param may have: Type $name desc  or just $name
        for word in rest.split_whitespace() {
            if word.starts_with('$') {
                existing.push(word.to_string());
                break;
            }
        }
    }

    existing
}

/// Check whether `@return` is already documented in the current docblock.
fn has_existing_return_tag(content: &str, position: Position) -> bool {
    use crate::docblock::TagKind;

    let byte_offset = position_to_byte_offset(content, position);
    let before_cursor = &content[..byte_offset.min(content.len())];

    let open_pos = match before_cursor.rfind("/**") {
        Some(pos) => pos,
        None => return false,
    };

    let docblock_so_far = &before_cursor[open_pos..];

    match crate::docblock::parser::parse_docblock_for_tags(docblock_so_far) {
        Some(info) => info.tags_by_kind(TagKind::Return).next().is_some(),
        None => false,
    }
}

/// Collect exception type names already documented with `@throws` tags
/// in the current docblock above the cursor.
///
/// Returns short type names as written in the docblock (e.g.
/// `"InvalidArgumentException"`, `"\\RuntimeException"`).
pub fn find_existing_throws_tags(content: &str, position: Position) -> Vec<String> {
    use crate::docblock::TagKind;

    let byte_offset = position_to_byte_offset(content, position);
    let before_cursor = &content[..byte_offset.min(content.len())];

    let open_pos = match before_cursor.rfind("/**") {
        Some(pos) => pos,
        None => return Vec::new(),
    };

    // Also look at the docblock text AFTER the cursor (the user may have
    // already documented some throws below the cursor line).
    let close_pos = content[open_pos..].find("*/").map(|p| open_pos + p + 2);
    let docblock = if let Some(end) = close_pos {
        &content[open_pos..end]
    } else {
        &content[open_pos..byte_offset.min(content.len())]
    };

    let info = match crate::docblock::parser::parse_docblock_for_tags(docblock) {
        Some(info) => info,
        None => return Vec::new(),
    };

    let mut existing = Vec::new();
    for tag in info.tags_by_kind(TagKind::Throws) {
        let rest = tag.description.trim();
        if let Some(type_name) = rest.split_whitespace().next() {
            let clean = type_name.trim_start_matches('\\');
            if !clean.is_empty() {
                existing.push(clean.to_string());
            }
        }
    }

    existing
}

// ─── Smart Completion Context ───────────────────────────────────────────────

/// Optional enrichment data for smart PHPDoc tag completion.
///
/// Bundles the inferred inline variable type and a class-loader callback
/// so that [`build_phpdoc_completions`] can pre-fill `@var`, `@param`,
/// and `@return` tags with concrete type information.
pub struct SmartContext<'a> {
    /// Pre-resolved type for an inline variable assignment (e.g.
    /// `list<int>` when the next line is `$items = [1, 2, 3];`).
    ///
    /// `None` when the type could not be inferred or the context is not
    /// an inline variable assignment.
    pub inferred_inline_var_type: Option<PhpType>,

    /// Callback that resolves a class name to its [`ClassInfo`], used
    /// to look up `@template` parameters for type enrichment.
    pub class_loader: Option<ClassLoaderFn<'a>>,

    /// Loads a class by its already-resolved name, for the types the
    /// `@throws` analysis works out.
    pub resolved_class_loader: Option<&'a crate::class_lookup::ResolvedClassLoader<'a>>,

    /// Callback that resolves a function name to its [`FunctionInfo`],
    /// used for cross-file `@throws` propagation from standalone
    /// function calls.
    pub function_loader: Option<FunctionLoaderFn<'a>>,

    /// The backend and file URI that plan `use` edits for `@throws`
    /// imports against the `namespace` block the cursor is in.  Without
    /// it the whole file is analysed.
    pub import_target: Option<(&'a crate::Backend, &'a str)>,

    /// The classes the file declares, which the body's `$this` and
    /// `self` name when its `@throws` are worked out.
    pub local_classes: &'a [Arc<ClassInfo>],
}

impl SmartContext<'_> {
    /// Empty context with no enrichment data.
    pub const EMPTY: SmartContext<'static> = SmartContext {
        inferred_inline_var_type: None,
        class_loader: None,
        resolved_class_loader: None,
        function_loader: None,
        import_target: None,
        local_classes: &[],
    };
}

// ─── Completion Builder ─────────────────────────────────────────────────────

/// Build completion items for PHPDoc tags based on context.
///
/// `content` is the full file text (used to extract symbol info and
/// detect already-documented parameters).
/// `prefix` is the partial tag the user has typed (e.g. `"@par"`, `"@"`).
/// `context` indicates what PHP symbol follows the docblock.
/// `position` is the cursor position (used to scan the docblock and the
/// following declaration).
/// `use_map` maps short class names to FQNs from `use` statements.
/// `file_namespace` is the file's declared namespace (if any).
/// `smart` carries optional enrichment data (inferred type, class loader).
///
/// Returns the list of matching `CompletionItem`s.
pub fn build_phpdoc_completions(
    content: &str,
    prefix: &str,
    context: DocblockContext,
    position: Position,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    smart: &SmartContext<'_>,
) -> Vec<CompletionItem> {
    let prefix_lower = prefix.to_lowercase();
    let site = TagSite {
        content,
        position,
        context,
        use_map,
        file_namespace,
        smart,
        sym: extract_symbol_info(content, position),
    };
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for def in tags_for_context(context).iter().copied().flatten() {
        if !def.tag.to_lowercase().starts_with(&prefix_lower) || !seen.insert(def.tag) {
            continue;
        }
        if !push_smart_tag_items(&site, def, &mut items) {
            items.push(generic_tag_item(def));
        }
    }
    items
}

/// Shorten a resolved type's class-like names using the file's `use`-map
/// and namespace, so a type that needs no enrichment still displays the
/// way the file would reference it (e.g. `Collection` instead of
/// `App\Collection` when the file has `use App\Collection;`).
fn shorten_php_type(
    ty: &PhpType,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> String {
    ty.resolve_names(&|name| crate::names::shorten_from_fqn(name, use_map, file_namespace))
        .to_string()
}
