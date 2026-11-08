//! Yii asset bundles: the files a bundle's `$css` and `$js` list.
//!
//! An entry that is not a URL names a file relative to the bundle's
//! `$sourcePath`, the directory `yiisoft/assets` publishes from, or, when
//! that is unset, its `$basePath`, where the files already are.  A
//! [`YiiAssetFile`](crate::symbol_map::StringKeyKind::YiiAssetFile) key holds both halves,
//! the directory with its alias left in and the entry as written (see
//! [`asset_file_key`]), so a diagnostic can tell an entry with no file from
//! a directory that is not there to judge it against.

use std::path::{Path, PathBuf};

use mago_syntax::cst::*;
use tower_lsp_server::gen_lsp_types::{Location, Position};

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::document_links::normalize_path;

use super::views::{is_absolute_view_path, normalize_view_path, path_value};

/// The class every asset bundle extends.
const ASSET_BUNDLE: &str = "Yiisoft\\Assets\\AssetBundle";

/// What separates the directory from the entry in an asset file key.  No
/// path a bundle names holds one.
const KEY_SEPARATOR: char = '\n';

/// Whether `class` extends `yiisoft/assets`' `AssetBundle` directly, with
/// `resolve` naming the class an `extends` identifier resolves to.
pub(crate) fn is_asset_bundle(
    class: &Class<'_>,
    resolve: impl Fn(&Identifier<'_>) -> String,
) -> bool {
    class.extends.as_ref().is_some_and(|extends| {
        extends
            .types
            .iter()
            .any(|parent| resolve(parent).trim_start_matches('\\') == ASSET_BUNDLE)
    })
}

/// The directory the entries of an asset bundle's `$css` and `$js` are
/// relative to, read from the defaults the class written in a file in
/// `dir` gives its properties: its `$sourcePath`, or its `$basePath` when
/// it gives no source path.  `None` when it gives neither in a form that
/// can be read, or when the bundle is served from a CDN, whose entries
/// name no file in the project.
pub(crate) fn bundle_dir(class: &Class<'_>, dir: &Path) -> Option<String> {
    let mut source = None;
    let mut base = None;
    for (name, value) in property_defaults(class) {
        match name {
            "$sourcePath" => source = Some(value),
            "$basePath" => base = Some(value),
            "$cdn" if matches!(value, Expression::Literal(Literal::True(_))) => return None,
            _ => {}
        }
    }
    // `AssetManager` publishes a bundle whose source path is not empty.
    let source = source.filter(|value| !is_empty_path(value));
    path_value(source.or(base)?, dir)
}

/// Whether a path property's default is `null` or `''`, which Yii reads as
/// no path.
fn is_empty_path(value: &Expression<'_>) -> bool {
    match value {
        Expression::Literal(Literal::Null(_)) => true,
        Expression::Literal(Literal::String(literal)) => {
            literal.value.is_some_and(|value| value.is_empty())
        }
        _ => false,
    }
}

/// The string literals that name a file in `class`'s `$css` and `$js`
/// defaults: each entry, or the first item of an entry that is an array
/// of the path and its options.
pub(crate) fn bundle_entries<'ast, 'arena>(
    class: &'ast Class<'arena>,
) -> impl Iterator<Item = &'ast Expression<'arena>> {
    property_defaults(class)
        .filter(|(name, _)| matches!(*name, "$css" | "$js"))
        .filter_map(|(_, value)| crate::parser::array_literal_elements(value))
        .flat_map(|elements| elements.iter())
        .filter_map(|element| {
            let value = match element {
                ArrayElement::Value(element) => element.value,
                ArrayElement::KeyValue(element) => element.value,
                _ => return None,
            };
            match crate::parser::array_literal_elements(value) {
                Some(options) => match options.iter().next()? {
                    ArrayElement::Value(first) => Some(first.value),
                    _ => None,
                },
                None => Some(value),
            }
        })
        .filter(|value| matches!(value, Expression::Literal(Literal::String(_))))
}

/// The defaults `class` gives the properties it declares, by name with the
/// `$`.
fn property_defaults<'ast, 'arena>(
    class: &'ast Class<'arena>,
) -> impl Iterator<Item = (&'ast str, &'ast Expression<'arena>)> {
    class
        .members
        .iter()
        .filter_map(|member| match member {
            ClassLikeMember::Property(Property::Plain(property)) => Some(property),
            _ => None,
        })
        .flat_map(|property| property.items.iter())
        .filter_map(|item| match item {
            PropertyItem::Concrete(concrete) => {
                Some((bytes_to_str(concrete.variable.name), concrete.value))
            }
            PropertyItem::Abstract(_) => None,
        })
}

/// Whether an entry names a file of the bundle's own: not a URL, which
/// `AssetUtil::isRelative()` tells apart, nor a path from the web root,
/// which `AssetLoader` passes through as is.
pub(crate) fn is_local_entry(entry: &str) -> bool {
    !entry.is_empty() && !entry.starts_with('/') && !entry.contains("://")
}

/// The key of the file `entry` names under the directory `dir`.
pub(crate) fn asset_file_key(dir: &str, entry: &str) -> Option<String> {
    is_local_entry(entry).then(|| format!("{dir}{KEY_SEPARATOR}{entry}"))
}

/// The directory and the entry of an asset file key.
pub(crate) fn split_asset_file_key(key: &str) -> (&str, &str) {
    key.split_once(KEY_SEPARATOR).unwrap_or(("", key))
}

impl Backend {
    /// The directory an asset file key's entry is relative to, with its
    /// alias replaced, or `None` when its alias is one the project does not
    /// declare.
    pub(crate) fn yii_asset_dir(&self, dir: &str) -> Option<PathBuf> {
        let resolved = self.yii_aliases().resolve(dir)?;
        is_absolute_view_path(&resolved).then(|| PathBuf::from(normalize_view_path(&resolved)))
    }

    /// The path an asset file key names, whether a file is there or not.
    fn yii_asset_path(&self, key: &str) -> Option<PathBuf> {
        let (dir, entry) = split_asset_file_key(key);
        Some(normalize_path(&self.yii_asset_dir(dir)?.join(entry)))
    }

    /// The file an asset file key names, when it exists.
    pub(crate) fn yii_asset_file(&self, key: &str) -> Option<PathBuf> {
        self.yii_asset_path(key).filter(|path| path.is_file())
    }

    /// Whether the file an asset file key names exists, or `None` when the
    /// directory it is relative to cannot be resolved or is not there, as
    /// the directory a bundle publishes to is not until it is published.
    pub(crate) fn yii_asset_file_exists(&self, key: &str) -> Option<bool> {
        let (dir, _) = split_asset_file_key(key);
        if !self.yii_asset_dir(dir)?.is_dir() {
            return None;
        }
        Some(self.yii_asset_file(key).is_some())
    }

    /// The file an asset file key names, as a diagnostic or hover spells
    /// it: relative to the workspace when it is under it.
    pub(crate) fn yii_asset_display(&self, key: &str) -> String {
        let Some(path) = self.yii_asset_path(key) else {
            return split_asset_file_key(key).1.to_string();
        };
        let root = self.workspace.workspace_root.read().clone();
        root.and_then(|root| {
            path.strip_prefix(&root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }

    /// Go-to-definition on an asset file key: the file it names.
    pub(crate) fn resolve_yii_asset_definitions(&self, key: &str) -> Vec<Location> {
        self.yii_asset_file(key)
            .and_then(|file| crate::util::path_to_url(&file))
            .map(|uri| crate::definition::point_location(uri, Position::new(0, 0)))
            .into_iter()
            .collect()
    }
}
