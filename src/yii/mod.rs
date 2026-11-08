//! Yii 3: the views an application renders and the path aliases it names
//! them through, the route names it declares and the arguments a handler
//! takes from its route's path, and the DI definitions it configures its
//! container with.
//!
//! A view is named by a path (`__DIR__ . '/template'`) or by a name the
//! renderer's view path prefixes (`render('index')` after
//! `withViewPath('@views/blog')`).  Symbol-map extraction
//! (`symbol_map/extraction/yii.rs`) spells each one as the path it names,
//! alias and all, as a [`YiiView`](crate::symbol_map::StringKeyKind::YiiView)
//! key, and [`views`] resolves that key to a template file through the
//! aliases the project's configuration declares (see [`aliases`]).
//!
//! A route is declared in PHP, by a `Route` definition in the arrays the
//! router's configuration returns, and read in PHP, so both sides are spans
//! in the symbol maps of PHP files (see `symbol_map/extraction/yii_routes.rs`).
//! The files that declare one are tracked with Symfony's (see
//! [`crate::symfony`]), and what a route's name alone does not say, its path
//! and its handler, is kept beside its span (see
//! [`crate::symbol_map::RouteSite`] and [`routes`]).
//!
//! A DI definition is recognised by its shape wherever it is written, and
//! its keys are spans on the class it builds (see [`definitions`]).
//!
//! The `$params` a configuration file is handed are merged from the params
//! files the merge plan lists, and the keys a file reads from them are
//! spans keyed by their path (see [`params`]).
//!
//! The `config-plugin` of a `composer.json` names those groups and lists
//! their files; it is read where it is written (see [`config_plugin`]).

pub(crate) mod aliases;
pub(crate) mod config_plugin;
pub(crate) mod definitions;
pub(crate) mod params;
pub(crate) mod routes;
pub(crate) mod views;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;

use crate::Backend;

/// The project's Yii path aliases, read on first use and dropped when a
/// configuration file changes.
#[derive(Default)]
pub(crate) struct YiiState {
    aliases: RwLock<Option<Arc<aliases::Aliases>>>,
    /// The DI definitions the installed packages contribute, read on first
    /// use and dropped when the dependencies change.
    package_definitions: parking_lot::Mutex<Option<Arc<Vec<definitions::PackageDefinitionFile>>>>,
    /// The project's merge plan, read on first use and dropped when a
    /// configuration file changes.
    config_plan: RwLock<Option<Arc<params::ConfigPlan>>>,
    /// The merged params, read on first use and dropped when a
    /// configuration file or the dependencies change.
    params: parking_lot::Mutex<Option<Arc<params::Params>>>,
    /// The names of the config groups the installed packages declare, read
    /// on first use and dropped when the dependencies change.
    package_groups: parking_lot::Mutex<Option<Arc<Vec<String>>>>,
}

/// The class names a string given to an HTML tag's `class()` names, each
/// with its byte offset in the string.  The string is written into the
/// tag's `class` attribute as it is, so every name separated by whitespace
/// is a class of its own.  A name written with an escape or built with
/// interpolation is not read.
pub(crate) fn css_class_names(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split(|c: char| c.is_ascii_whitespace())
        .filter(|name| !name.is_empty() && !name.contains(['\\', '$', '{', '}']))
        .map(move |name| (name.as_ptr() as usize - text.as_ptr() as usize, name))
}

impl Backend {
    /// Whether the project is a Yii application; see
    /// [`Backend::is_yii`](crate::Backend::is_yii).
    pub(crate) fn is_yii_project(&self) -> bool {
        self.is_yii.load(Ordering::Relaxed)
    }

    /// Record whether the project is a Yii application, once the workspace's
    /// `composer.json` files have been read.
    pub(crate) fn set_is_yii(&self, is_yii: bool) {
        self.is_yii.store(is_yii, Ordering::Relaxed);
    }

    /// The path aliases the project's configuration declares.
    pub(crate) fn yii_aliases(&self) -> Arc<aliases::Aliases> {
        if let Some(aliases) = self.yii.aliases.read().as_ref() {
            return Arc::clone(aliases);
        }
        let root = self.workspace.workspace_root.read().clone();
        let aliases = Arc::new(
            root.map(|root| aliases::Aliases::load(&root))
                .unwrap_or_default(),
        );
        *self.yii.aliases.write() = Some(Arc::clone(&aliases));
        aliases
    }

    /// Forget the aliases and the params when the file at `uri` may
    /// declare some, so the next lookup reads them again.
    pub(crate) fn note_yii_config_change(&self, uri: &str) {
        if self.is_yii_project() && uri.contains("/config/") && uri.ends_with(".php") {
            *self.yii.aliases.write() = None;
            self.forget_yii_params();
        }
    }
}
