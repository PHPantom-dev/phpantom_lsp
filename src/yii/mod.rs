//! Yii 3: the views an application renders, and the path aliases it names
//! them through.
//!
//! A view is named by a path (`__DIR__ . '/template'`) or by a name the
//! renderer's view path prefixes (`render('index')` after
//! `withViewPath('@views/blog')`).  Symbol-map extraction
//! (`symbol_map/extraction/yii.rs`) spells each one as the path it names,
//! alias and all, as a [`YiiView`](crate::symbol_map::StringKeyKind::YiiView)
//! key, and [`views`] resolves that key to a template file through the
//! aliases the project's configuration declares (see [`aliases`]).

pub(crate) mod aliases;
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
}

impl Backend {
    /// Whether the project renders Yii views; see
    /// [`Backend::is_yii`](crate::Backend::is_yii).
    pub(crate) fn is_yii_project(&self) -> bool {
        self.is_yii.load(Ordering::Relaxed)
    }

    /// Record whether the project renders Yii views, once the workspace's
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

    /// Forget the aliases when the file at `uri` may declare some, so the
    /// next lookup reads them again.
    pub(crate) fn note_yii_config_change(&self, uri: &str) {
        if self.is_yii_project() && uri.contains("/config/") && uri.ends_with(".php") {
            *self.yii.aliases.write() = None;
        }
    }
}
