//! Yii 3: the views an application renders and the path aliases it names
//! them through, the route names it declares and the arguments a handler
//! takes from its route's path, the DI definitions it configures its
//! container with, the listeners it attaches to its events, and the
//! properties its form fields and hydration maps name.
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
//! The files an asset bundle's `$css` and `$js` list are spans keyed by
//! the directory the bundle names and the entry (see [`assets`]).
//!
//! The `config-plugin` of a `composer.json` names those groups and lists
//! their files; it is read where it is written (see [`config_plugin`]).
//!
//! The listeners the events configuration attaches to an event class are
//! read from the files of the events groups, the project's and the
//! installed packages' (see [`events`]).
//!
//! The message IDs a `translate()` call names are spans keyed by the
//! category they are looked up in, and are declared by the message files
//! of the directories the DI definitions give their message sources (see
//! [`translations`]).
//!
//! The attributes and relations an Active Record names as strings are read
//! against the record (see [`active_record`]), and the attributes of a
//! record that declares no properties against the columns the project's
//! migrations give its table (see [`migrations`]).
//!
//! The properties a form field and a hydration map name are member
//! accesses on the object they are given (see [`forms`]).
//!
//! The key paths `ArrayHelper` reads are walked through the value they
//! read (see [`array_helper`]).

pub(crate) mod active_record;
pub(crate) mod aliases;
pub(crate) mod array_helper;
pub(crate) mod assets;
pub(crate) mod config_plugin;
pub(crate) mod definitions;
pub(crate) mod events;
pub(crate) mod forms;
pub(crate) mod migrations;
pub(crate) mod params;
pub(crate) mod relations;
pub(crate) mod routes;
pub(crate) mod translations;
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
    /// The event listeners the installed packages attach, read on first
    /// use and dropped when the dependencies change.
    package_listeners: parking_lot::Mutex<Option<Arc<Vec<events::PackageListenerFile>>>>,
    /// The project's merge plan, read on first use and dropped when a
    /// configuration file changes.
    config_plan: RwLock<Option<Arc<params::ConfigPlan>>>,
    /// The merged params, read on first use and dropped when a
    /// configuration file or the dependencies change.
    params: parking_lot::Mutex<Option<Arc<params::Params>>>,
    /// The names of the config groups the installed packages declare, read
    /// on first use and dropped when the dependencies change.
    package_groups: parking_lot::Mutex<Option<Arc<Vec<String>>>>,
    /// The message files, read on first use and dropped when a
    /// configuration file, a message file, or the dependencies change.
    messages: parking_lot::Mutex<Option<Arc<translations::Messages>>>,
    /// The directories the message files were last read from.
    message_dirs: RwLock<Vec<std::path::PathBuf>>,
    /// The tables the project's migrations create, read on first use and
    /// dropped when a configuration file, a migration, or the dependencies
    /// change.
    schema: parking_lot::Mutex<Option<Arc<migrations::Schema>>>,
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

    /// Forget the aliases, the params, the message files, and the tables
    /// when the file at `uri` may declare some, so the next lookup reads
    /// them again.
    pub(crate) fn note_yii_config_change(&self, uri: &str) {
        if !self.is_yii_project() {
            return;
        }
        if uri.contains("/config/") && uri.ends_with(".php") {
            *self.yii.aliases.write() = None;
            self.forget_yii_params();
            self.forget_yii_messages();
            self.forget_yii_schema();
        } else if let Some(path) = crate::util::uri_to_path(uri) {
            self.note_yii_message_change(&path);
            self.note_yii_migration_change(&path);
        }
    }
}
