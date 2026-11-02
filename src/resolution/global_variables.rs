//! The type the `@global` tags of library and project code give a global
//! variable, which a `global $x;` with no type of its own takes.

use std::path::PathBuf;

use crate::Backend;
use crate::names::qualify_written_type_names;
use crate::php_type::PhpType;
use crate::text_scan::NamespaceDecls;

impl Backend {
    /// The type the `@global` tags the discovery scans found give the
    /// global variable `name` (with its `$`), joined over every tag that
    /// documents it, or `None` when none does.
    ///
    /// A tag documents what a function reads from the global, which is
    /// what the global holds wherever it is imported, so a tag on one
    /// function counts for every other.  When a stub path documents the
    /// global, only its tags count, the way its declarations win over
    /// every other.  Each type is read with the namespace and imports of
    /// the file it is written in.
    pub(crate) fn load_global_tag_type(&self, name: &str) -> Option<PhpType> {
        let (mut paths, generation) = {
            let tags = self.symbols.global_tags.read();
            if let Some(cached) = tags.cached_type(name) {
                return cached.clone();
            }
            let paths: Vec<PathBuf> = tags.files(name)?.iter().cloned().collect();
            (paths, tags.generation())
        };
        if self.has_external_stubs() {
            let stubs = self.external_stubs();
            if paths.iter().any(|path| stubs.contains_path(path)) {
                paths.retain(|path| stubs.contains_path(path));
            }
        }

        let mut members: Vec<PhpType> = Vec::new();
        for path in &paths {
            let Some(content) = crate::util::read_php_source(path) else {
                continue;
            };
            let tagged = crate::docblock::docblocks_with_global_tags(&content);
            let namespaces = NamespaceDecls::scan(&content);
            // The file is only parsed for its imports when it may have some.
            let imports = if has_use_statement(&content) {
                self.parse_use_statements(&content)
            } else {
                Default::default()
            };
            for (start, doc) in tagged {
                for (tag_name, php_type) in crate::docblock::extract_global_tags(doc) {
                    if tag_name != name {
                        continue;
                    }
                    let namespace = namespaces.at(&content, start);
                    let qualified = qualify_written_type_names(&php_type, namespace, &imports);
                    members.extend(qualified.union_members().into_iter().cloned());
                }
            }
        }
        let php_type = (!members.is_empty()).then(|| PhpType::union(members));
        self.symbols
            .global_tags
            .write()
            .cache_type(name, php_type.clone(), generation);
        php_type
    }
}

/// Whether a line of `content` starts with the `use` keyword, which an
/// import does and the prose of a docblock does not.  A trait's `use`
/// inside a class body passes too, which only costs a parse.
fn has_use_statement(content: &str) -> bool {
    content.lines().any(|line| {
        line.trim_start()
            .strip_prefix("use")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_whitespace()))
    })
}
