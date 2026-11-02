//! Loading what the stub paths declare, the step each lookup takes before
//! any other.
//!
//! A stub file is parsed like any other file, under its own `file://` URI,
//! so go-to-definition lands in it and the file watcher's re-parse covers
//! it. What makes it win is that these loaders read the declaration out
//! of the stub file itself rather than out of the shared indexes, where a
//! project or vendor declaration of the same name may be the one in front.

use std::sync::Arc;

use super::inflight::InflightGuard;
use crate::Backend;
use crate::names::qualify_written_type_names;
use crate::php_type::PhpType;
use crate::text_scan::NamespaceDecls;
use crate::types::{ClassInfo, DefineInfo, FunctionInfo};

impl Backend {
    /// The class `class_name` names in the stub paths, or `None` when no
    /// stub path declares it.
    ///
    /// `class_name` is a fully-qualified name without the leading `\`.
    pub(crate) fn load_external_stub_class(&self, class_name: &str) -> Option<Arc<ClassInfo>> {
        if !self.has_external_stubs() {
            return None;
        }
        let path = self.external_stubs.read().classes.get(class_name)?.clone();
        // Parsed with its functions and constants, which a project that
        // uses a stub package's classes reaches for too. Caching only the
        // classes here would leave the first function lookup to parse the
        // whole file a second time.
        let uri = self.parse_external_stub_globals(&path)?;
        let classes = self.symbols.uri_classes_index.read().get(&uri).cloned()?;
        classes
            .iter()
            .find(|class| class.fqn().eq_ignore_ascii_case(class_name))
            .map(Arc::clone)
    }

    /// The function the first of `candidates` a stub path declares names
    /// there, or `None` when no stub path declares any of them.
    pub(super) fn load_external_stub_function(&self, candidates: &[&str]) -> Option<FunctionInfo> {
        if !self.has_external_stubs() {
            return None;
        }
        let stubs = self.external_stubs();
        let (name, path) = candidates
            .iter()
            .find_map(|&name| stubs.functions.get_key_value(name))?;
        let uri = self.parse_external_stub_globals(path)?;
        let mut function = self.function_declared_by(name, &uri)?;
        if let Some(wordpress) = &stubs.wordpress {
            wordpress.apply(path, &mut function);
        }
        Some(function)
    }

    /// What `read` takes from the constant the first of `candidates` a
    /// stub path declares, or `None` when no stub path declares any of
    /// them.
    pub(super) fn load_external_stub_define<T>(
        &self,
        candidates: &[&str],
        read: impl Fn(&DefineInfo) -> T,
    ) -> Option<T> {
        if !self.has_external_stubs() {
            return None;
        }
        let (name, path) = {
            let stubs = self.external_stubs.read();
            candidates.iter().find_map(|&name| {
                stubs
                    .constants
                    .get_key_value(name)
                    .map(|(name, path)| (name.clone(), path.clone()))
            })?
        };
        let uri = self.parse_external_stub_globals(&path)?;
        self.symbols
            .global_defines
            .read()
            .get(&name)
            .filter(|define| define.file_uri == uri)
            .map(read)
    }

    /// The type the `@global` tags of the stub paths give the global
    /// variable `name` (with its `$`), joined over every tag that
    /// documents it, or `None` when none does.
    ///
    /// A tag documents what a function reads from the global, which is
    /// what the global holds wherever it is imported. Each type is read
    /// with the imports of the file it is written in.
    pub(crate) fn load_external_stub_global(&self, name: &str) -> Option<PhpType> {
        if !self.has_external_stubs() {
            return None;
        }
        let stubs = self.external_stubs();
        let paths = stubs.globals.get(name)?;
        if let Some(cached) = stubs.global_types.lock().get(name) {
            return cached.clone();
        }
        let mut members: Vec<PhpType> = Vec::new();
        for path in paths {
            let Some(uri) = self.parse_external_stub_globals(path) else {
                continue;
            };
            let Some(content) = self.get_file_content_arc(&uri) else {
                continue;
            };
            let imports = self.file_use_map(&uri);
            let namespaces = NamespaceDecls::scan(&content);
            for (start, doc) in crate::docblock::docblocks_with_global_tags(&content) {
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
        stubs
            .global_types
            .lock()
            .insert(name.to_owned(), php_type.clone());
        php_type
    }

    /// Parse the stub file at `path` for its classes, functions and
    /// constants, unless it already was, and return its URI.
    ///
    /// The parse leaves the file's state behind to say it happened.
    /// Claimed like any other lazy parse, so the workers of a diagnostic
    /// pass that all reach for built-ins at once parse each large stub
    /// file once rather than once per worker.
    fn parse_external_stub_globals(&self, path: &std::path::Path) -> Option<String> {
        let uri = crate::util::path_to_uri(path);
        let parsed = || self.files.read().contains_key(&uri);
        if parsed() {
            return Some(uri);
        }
        let _guard = if self.parse_inflight.try_claim(&uri) {
            Some(InflightGuard {
                inflight: &self.parse_inflight,
                uri: uri.clone(),
            })
        } else {
            // The claim may be a class lookup's, which leaves the globals
            // to this parse.
            self.parse_inflight.wait_until_released(&uri);
            None
        };
        if !parsed() {
            let content = crate::util::read_php_source(path)?;
            self.update_ast(&uri, &content);
        }
        Some(uri)
    }
}
