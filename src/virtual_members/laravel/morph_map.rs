//! Static scan of Laravel's polymorphic morph map
//! (`Relation::morphMap()` / `Relation::enforceMorphMap()`).
//!
//! Eloquent stores the `*_type` column value for a polymorphic relation as the
//! related model's class name unless the application registers a morph map,
//! which substitutes a short alias (`'post'`) for the FQCN
//! (`App\Models\Post`).  The map is a runtime static on
//! `Illuminate\Database\Eloquent\Relations\Relation`, populated from a service
//! provider's `boot()`; the registration itself is a literal fact in provider
//! source, recoverable the same way the `::macro()` scanner recovers macros.
//!
//! This module extracts registrations of the shape
//! `Relation::morphMap(['post' => Post::class, …])` (and the `enforceMorphMap`
//! variant, which additionally requires every morphed model to be mapped) into
//! an alias → FQCN table.  Laravel's list shorthand
//! `Relation::morphMap([Post::class, Video::class])` derives each alias from
//! the model's table name, which a single-file scan cannot know; those entries
//! are reported separately as [`MorphMapScan::table_keyed`] so the caller can
//! resolve the table through the class index.
//!
//! Registrations whose map argument is not a literal array, or whose values are
//! not `::class` constants, are skipped — a partial map is still useful, and an
//! unknown alias simply keeps resolving to nothing rather than to a guess.

use std::collections::HashMap;

use mago_allocator::LocalArena;
use mago_database::file::FileId;
use mago_names::resolver::NameResolver;
use mago_span::HasSpan;
use mago_syntax::cst::*;
use mago_syntax::parser::parse_file_content;

use super::file_contributions::{Contribution, FileContributions};
use super::helpers::string_literal_at;
use crate::atom::bytes_to_str;
use crate::names::OwnedResolvedNames;

/// FQN of the class that owns the morph map.
const RELATION_FQN: &str = "Illuminate\\Database\\Eloquent\\Relations\\Relation";

/// A single `'alias' => Model::class` pair recovered from a morph-map
/// registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MorphMapEntry {
    /// The morph alias, i.e. the value stored in the `*_type` column.
    pub alias: String,
    /// FQN of the model the alias maps to, resolved via the file's `use`
    /// statements.
    pub target_fqn: String,
    /// Byte offset of the alias string literal's content (just inside the
    /// opening quote) in the file the registration was found in.
    pub alias_offset: u32,
}

/// A `Relation::morphMap([Model::class, …])` list entry, whose alias is the
/// model's table name rather than a literal in source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TableKeyedTarget {
    /// FQN of the model, resolved via the file's `use` statements.
    pub target_fqn: String,
    /// Byte offset of the `Model::class` expression, used as the registration
    /// location for the derived alias.
    pub offset: u32,
}

/// Everything one file contributes to the morph map.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MorphMapScan {
    /// `'alias' => Model::class` pairs, ready to index.
    pub entries: Vec<MorphMapEntry>,
    /// List-shorthand targets whose alias is the model's table name.
    pub table_keyed: Vec<TableKeyedTarget>,
    /// Whether the file calls `enforceMorphMap()` or `requireMorphMap()`,
    /// which makes the map the exhaustive set of morphable models.
    pub enforced: bool,
}

impl Contribution for MorphMapScan {
    fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.table_keyed.is_empty() && !self.enforced
    }
}

/// Extract every literal morph-map registration from a file's source.
///
/// Returns an empty scan when the file contains neither `orphMap(` (the shared
/// tail of `morphMap(` and `enforceMorphMap(`) nor `requireMorphMap(`, so the
/// parse is only paid for candidate files.
pub(crate) fn scan_morph_map(content: &str) -> MorphMapScan {
    let bytes = content.as_bytes();
    if memchr::memmem::find(bytes, b"orphMap(").is_none() {
        return MorphMapScan::default();
    }

    let arena = LocalArena::new();
    let file_id = FileId::new(b"input.php");
    let program = parse_file_content(&arena, file_id, bytes);
    let resolved = NameResolver::new(&arena).resolve(program);
    let owned = OwnedResolvedNames::from_resolved(&resolved);

    let mut scan = MorphMapScan::default();
    collect_morph_map_calls(Node::Program(program), &owned, content, &mut scan);
    scan
}

/// Recursively collect every `Relation::morphMap(...)` /
/// `Relation::enforceMorphMap(...)` / `Relation::requireMorphMap(...)` call.
fn collect_morph_map_calls(
    node: Node<'_, '_>,
    resolved: &OwnedResolvedNames,
    content: &str,
    scan: &mut MorphMapScan,
) {
    if let Node::StaticMethodCall(smc) = node
        && let ClassLikeMemberSelector::Identifier(ident) = &smc.method
        && let Some(method) = morph_map_method(bytes_to_str(ident.value))
        && subject_is_relation(smc.class, resolved)
    {
        if method.enforces {
            scan.enforced = true;
        }
        if method.takes_map
            && let Some(arg) = smc.argument_list.arguments.first()
        {
            collect_map_argument(arg.value(), resolved, content, scan);
        }
    }
    node.visit_children(|child| collect_morph_map_calls(child, resolved, content, scan));
}

/// Which morph-map API a method name refers to.
struct MorphMapMethod {
    /// Whether the call marks the map as exhaustive.
    enforces: bool,
    /// Whether the first argument is the alias → model map.
    takes_map: bool,
}

fn morph_map_method(name: &str) -> Option<MorphMapMethod> {
    if name.eq_ignore_ascii_case("morphMap") {
        Some(MorphMapMethod {
            enforces: false,
            takes_map: true,
        })
    } else if name.eq_ignore_ascii_case("enforceMorphMap") {
        Some(MorphMapMethod {
            enforces: true,
            takes_map: true,
        })
    } else if name.eq_ignore_ascii_case("requireMorphMap") {
        Some(MorphMapMethod {
            enforces: true,
            takes_map: false,
        })
    } else {
        None
    }
}

/// Whether the class written before `::morphMap` resolves to Eloquent's
/// `Relation`.
///
/// Requiring the resolved FQN (rather than matching the short name) means an
/// unrelated `Relation` class in the current namespace is not mistaken for
/// Eloquent's, while an aliased import (`use … Relation as EloquentRelation`)
/// still matches.
fn subject_is_relation(class: &Expression<'_>, resolved: &OwnedResolvedNames) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let raw = bytes_to_str(ident.value());
    let fqn = resolved
        .get(ident.span().start.offset)
        .map(|fqn| fqn.trim_start_matches('\\').to_string())
        .unwrap_or_else(|| raw.trim_start_matches('\\').to_string());
    fqn.eq_ignore_ascii_case(RELATION_FQN)
}

/// Read the alias → model pairs (or list shorthand entries) out of a
/// `morphMap()` argument.  Anything but a literal array is ignored.
fn collect_map_argument(
    expr: &Expression<'_>,
    resolved: &OwnedResolvedNames,
    content: &str,
    scan: &mut MorphMapScan,
) {
    let elements = match expr {
        Expression::Array(array) => &array.elements,
        Expression::LegacyArray(array) => &array.elements,
        _ => return,
    };

    for element in elements.iter() {
        match element {
            ArrayElement::KeyValue(kv) => {
                let Some(target_fqn) = class_constant_fqn(kv.value, resolved, content) else {
                    continue;
                };
                let Some((alias, alias_offset)) = string_literal_at(kv.key, content) else {
                    continue;
                };
                scan.entries.push(MorphMapEntry {
                    alias: alias.to_string(),
                    target_fqn,
                    alias_offset,
                });
            }
            ArrayElement::Value(value) => {
                if let Some(target_fqn) = class_constant_fqn(value.value, resolved, content) {
                    scan.table_keyed.push(TableKeyedTarget {
                        target_fqn,
                        offset: value.value.span().start.offset,
                    });
                }
            }
            ArrayElement::Variadic(_) | ArrayElement::Missing(_) => {}
        }
    }
}

/// The FQN behind a `Model::class` expression, resolved via the file's `use`
/// statements.  A plain string literal is accepted too, since
/// `['post' => 'App\Models\Post']` is a legal (if unidiomatic) map value; its
/// escaped separators are normalized so both quoting styles yield the same FQN.
fn class_constant_fqn(
    expr: &Expression<'_>,
    resolved: &OwnedResolvedNames,
    content: &str,
) -> Option<String> {
    match expr {
        Expression::Access(Access::ClassConstant(access))
            if matches!(
                &access.constant,
                ClassLikeConstantSelector::Identifier(constant)
                    if bytes_to_str(constant.value).eq_ignore_ascii_case("class")
            ) =>
        {
            let Expression::Identifier(ident) = access.class else {
                return None;
            };
            let raw = bytes_to_str(ident.value());
            if matches!(
                raw.to_ascii_lowercase().as_str(),
                "self" | "static" | "parent"
            ) {
                return None;
            }
            resolved
                .get(ident.span().start.offset)
                .map(|fqn| fqn.trim_start_matches('\\').to_string())
                .or_else(|| (!raw.is_empty()).then(|| raw.trim_start_matches('\\').to_string()))
        }
        Expression::Literal(Literal::String(_)) => {
            let (raw, _) = string_literal_at(expr, content)?;
            let fqn = raw.replace("\\\\", "\\");
            let fqn = fqn.trim_start_matches('\\');
            (!fqn.is_empty()).then(|| fqn.to_string())
        }
        _ => None,
    }
}

/// Where a morph alias was registered, and what it maps to.
#[derive(Clone, Debug)]
pub(crate) struct MorphTarget {
    /// FQN of the mapped model.
    pub fqn: String,
    /// URI of the file holding the registration.
    pub uri: String,
    /// Byte offset of the alias literal (or, for the list shorthand, of the
    /// `Model::class` expression) within that file.
    pub offset: u32,
}

/// Project-wide index of Laravel morph-map registrations.
///
/// Stored on [`Backend`](crate::Backend) and built for Laravel projects after
/// indexing.  `files` is the source of truth (one entry per contributing file,
/// so an edit to a file can replace just that file's registrations); `aliases`
/// and `by_fqn` are the derived lookup maps.
#[derive(Default)]
pub(crate) struct LaravelMorphMapIndex {
    pub(crate) files: FileContributions<MorphMapScan>,
    /// Alias → mapped model, merged across every contributing file.
    aliases: HashMap<String, MorphTarget>,
    /// Whether any provider called `enforceMorphMap()` / `requireMorphMap()`,
    /// which makes [`aliases`](Self::aliases) the exhaustive set of morph types.
    enforced: bool,
}

impl LaravelMorphMapIndex {
    /// Rebuild the derived lookup maps from the per-file scans.
    ///
    /// A duplicate alias keeps the first registration seen.  Laravel's own
    /// `morphMap($map, $merge = true)` lets a later call override an earlier
    /// one, but scan order across files is not the runtime boot order, so
    /// first-wins is the stable choice (matching the macro index).
    pub(crate) fn rebuild(&mut self) {
        let mut aliases: HashMap<String, MorphTarget> = HashMap::new();
        let mut enforced = false;

        for (uri, scan) in self.files.iter() {
            enforced |= scan.enforced;
            for entry in &scan.entries {
                aliases
                    .entry(entry.alias.clone())
                    .or_insert_with(|| MorphTarget {
                        fqn: entry.target_fqn.clone(),
                        uri: uri.clone(),
                        offset: entry.alias_offset,
                    });
            }
        }

        self.aliases = aliases;
        self.enforced = enforced;
    }

    /// The model an alias maps to, if registered.
    pub(crate) fn get(&self, alias: &str) -> Option<&MorphTarget> {
        self.aliases.get(alias)
    }

    /// Every registered alias.
    pub(crate) fn all_aliases(&self) -> Vec<String> {
        self.aliases.keys().cloned().collect()
    }

    /// Whether `alias` is registered.
    pub(crate) fn has_alias(&self, alias: &str) -> bool {
        self.aliases.contains_key(alias)
    }

    /// Whether a provider called `enforceMorphMap()` / `requireMorphMap()`, so
    /// that every morphable model must appear in the map.
    pub(crate) fn is_enforced(&self) -> bool {
        self.enforced
    }
}

#[cfg(test)]
#[path = "morph_map_tests.rs"]
mod tests;
