//! WordPress: what the engine supplies that the WordPress stubs leave out.
//!
//! `php-stubs/wordpress-stubs` declares core's functions and classes, but
//! not the constants core `define()`s at runtime. Some of those carry the
//! stubs' own types: `get_post($post, $output = OBJECT)` declares its return
//! type conditional on `$output`, and a default it cannot read leaves every
//! branch standing.

use crate::Backend;
use crate::stubs::STUB_URI_PREFIX;

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
#[path = "wordpress_tests.rs"]
mod tests;
