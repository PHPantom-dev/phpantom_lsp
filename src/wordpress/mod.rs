//! WordPress: what the engine supplies that the WordPress stubs leave out.
//!
//! `php-stubs/wordpress-stubs` declares core's functions and classes, but
//! not the constants core `define()`s at runtime. Some of those carry the
//! stubs' own types: `get_post($post, $output = OBJECT)` declares its return
//! type conditional on `$output`, and a default it cannot read leaves every
//! branch standing.
//!
//! Others leave a return type an argument decides at the type every
//! argument shares: `get_post_meta($id, $key, true)` is declared `mixed`
//! whether `$single` asks for one value or for all of them. `overrides.php`
//! declares the narrower types, and they replace the stubs' own. A return
//! type no PHPDoc type can spell (the shape `wp_parse_args()` merges, the
//! literal `trailingslashit('foo')` returns) is read off the call's
//! arguments in `type_engine::type_ops::call_site_returns`.

use std::path::{Path, PathBuf};

use crate::Backend;
use crate::ci_map::CiMap;
use crate::php_type::PhpType;
use crate::stubs::STUB_URI_PREFIX;
use crate::types::FunctionInfo;

/// The return types `overrides.php` gives functions of the WordPress stubs.
const OVERRIDES: &str = include_str!("overrides.php");

/// The stub file that declares WordPress, and the return types that replace
/// its own.
pub(crate) struct WordPressStubs {
    file: PathBuf,
    /// Keyed by function name: `return_type`, then `conditional_return`.
    returns: CiMap<(Option<PhpType>, Option<PhpType>)>,
}

impl WordPressStubs {
    /// Give `function`, as the stub file at `path` declares it, the return
    /// type `overrides.php` declares for it, where `path` is the file that
    /// declares WordPress.
    ///
    /// Another stub file's declaration of the name is one the user chose
    /// over the WordPress stubs, and keeps its own.
    pub(crate) fn apply(&self, path: &Path, function: &mut FunctionInfo) {
        if path != self.file || function.namespace.is_some() {
            return;
        }
        if let Some((return_type, conditional_return)) = self.returns.get(&function.name) {
            // An override that declares only a conditional keeps the stubs'
            // `@return`, which is what hover shows for the declaration.
            if return_type.is_some() {
                function.return_type.clone_from(return_type);
            }
            function.conditional_return.clone_from(conditional_return);
        }
    }
}

/// The output formats `wpdb` and the functions built on it take
/// (`get_post($id, ARRAY_A)`), as core's `wp-includes/class-wpdb.php`
/// defines them, each in a source of its own the way an embedded stub
/// constant is.
const OUTPUT_CONSTANTS: &[(&str, &str)] = &[
    ("OBJECT", "<?php\ndefine('OBJECT', 'OBJECT');\n"),
    ("OBJECT_K", "<?php\ndefine('OBJECT_K', 'OBJECT_K');\n"),
    ("ARRAY_A", "<?php\ndefine('ARRAY_A', 'ARRAY_A');\n"),
    ("ARRAY_N", "<?php\ndefine('ARRAY_N', 'ARRAY_N');\n"),
];

impl Backend {
    /// The WordPress stubs among the stub files whose classes `classes`
    /// indexes, the file that declares `wpdb`, or `None` when none does.
    pub(crate) fn wordpress_stubs(&self, classes: &CiMap<PathBuf>) -> Option<WordPressStubs> {
        let file = classes.get("wpdb")?.clone();
        let mut returns = CiMap::new();
        for function in self.parse_functions(OVERRIDES) {
            returns.insert(
                function.name.as_str(),
                (function.return_type, function.conditional_return),
            );
        }
        Some(WordPressStubs { file, returns })
    }

    /// Offer WordPress's output constants as built-in constants while the
    /// stub paths declare WordPress (its `wpdb` class), and withdraw them
    /// once they no longer do.
    ///
    /// A project that indexes WordPress core itself reads the constants
    /// from core's own `define()`s, and a stub path that declares them
    /// (`php-stubs/wordpress-globals`) wins over these.
    pub(crate) fn sync_wordpress_constants(&self, stubs_declare_wordpress: bool) {
        let mut index = self.stub_constant_index.write();
        for &(name, source) in OUTPUT_CONSTANTS {
            if stubs_declare_wordpress {
                index.entry(name).or_insert(source);
            } else if index.get(name).is_some_and(|s| std::ptr::eq(*s, source)) {
                index.remove(name);
                drop_parsed_stub_constant(self, name);
            }
        }
    }
}

/// Forget what parsing the embedded source of the constant `name` recorded.
fn drop_parsed_stub_constant(backend: &Backend, name: &str) {
    let uri = format!("{STUB_URI_PREFIX}const/{name}");
    let parsed = backend
        .symbols
        .global_defines
        .read()
        .get(name)
        .is_some_and(|info| info.file_uri == uri);
    if parsed {
        backend.symbols.global_defines.write().remove(name);
        backend.clear_file_maps(&uri);
    }
}

#[cfg(test)]
mod tests;
