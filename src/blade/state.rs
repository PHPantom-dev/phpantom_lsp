//! The Blade state a `Backend` keeps beyond each template's own lowering
//! (see [`FileState`](crate::file_state::FileState)).

use std::collections::{HashMap, HashSet};

use parking_lot::RwLock;

/// What the server knows about the project's Blade templates.
#[derive(Default)]
pub(crate) struct BladeState {
    /// The Blade directives the project's templates can write: the built-in
    /// ones the installed Blade compiler defines, and the ones its providers
    /// register, expanded from the merged table's `custom_directives` into
    /// every name a template can write.  Kept separate from the table
    /// because the Blade preprocessor reads it on every keystroke in a
    /// template and must not pay for the expansion each time.
    pub(crate) directives: RwLock<super::directives::BladeDirectives>,
    /// URIs opened with `languageId == "blade"` that don't have a `.blade.php` extension.
    /// Allows editors to signal Blade files via languageId alone.
    pub(crate) uris: RwLock<HashSet<String>>,
    /// Per-template scope seeded into the virtual PHP on the last
    /// preprocess: the declared and call-site-inferred variables, and the
    /// class `$this` is bound to.  Lets re-inference passes skip templates
    /// whose scope is unchanged.
    pub(crate) injected_vars: RwLock<HashMap<String, super::call_site_inference::BladeScope>>,
    /// Per-file view spans whose render site only the receiver's *type*
    /// settles, computed on first use and dropped when the file is
    /// re-parsed (see [`crate::blade::typed_receiver`]).  Only files whose
    /// symbol map recorded a candidate site ever get an entry.
    pub(crate) typed_receiver_view_spans_cache:
        RwLock<HashMap<String, super::typed_receiver::TypedReceiverSpans>>,
}
