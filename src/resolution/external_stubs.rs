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
        let uri = crate::util::path_to_uri(&path);
        let cached = self.symbols.uri_classes_index.read().get(&uri).cloned();
        let classes = match cached {
            Some(classes) => classes,
            None => self.parse_and_cache_file(&path)?,
        };
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
        let (name, path) = {
            let stubs = self.external_stubs.read();
            candidates.iter().find_map(|&name| {
                stubs
                    .functions
                    .get_key_value(name)
                    .map(|(name, path)| (name.to_owned(), path.clone()))
            })?
        };
        let uri = self.parse_external_stub_globals(&path)?;
        self.function_declared_by(&name, &uri)
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

    /// Parse the stub file at `path` for its functions and constants,
    /// unless it already was, and return its URI.
    ///
    /// A file a class lookup parsed counts as not parsed here: that parse
    /// only caches classes, while this one publishes the functions and
    /// constants too, and leaves the file's state behind to say so.
    ///
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
