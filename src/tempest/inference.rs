//! Data passed to Tempest views, resolved in each caller's PHP scope.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::Backend;
use crate::php_type::{PhpType, ShapeEntry};
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolMap};

/// Inferred declarations, kept below annotations written in the template.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scope {
    /// Locals extracted or passed to this template invocation.
    pub(super) vars: BTreeMap<String, PhpType>,
    /// The renderer proxy's object, independent of component attributes.
    pub(super) bound_this: Option<PhpType>,
    /// Isolated slot scopes, keyed by the opening tag's source offset.
    pub(super) regions: BTreeMap<usize, Scope>,
    /// View data and direct component loop bindings forwarded to children.
    pub(super) globals: BTreeMap<String, PhpType>,
    /// Variables read by supplied slot bodies at each component outlet.
    pub(super) slot_reads: BTreeMap<usize, BTreeSet<String>>,
}

impl Scope {
    /// Join independent invocations in a deterministic order.
    pub(super) fn merge(&mut self, other: Self) {
        self.bound_this = match (self.this_type(), other.this_type()) {
            (Some(left), Some(right)) => Some(
                crate::blade::call_site_inference::join_call_site_types(vec![left, right]),
            ),
            (left, right) => left.or(right),
        };
        for (outlet, names) in other.slot_reads {
            self.slot_reads.entry(outlet).or_default().extend(names);
        }
        for (dest, source) in [
            (&mut self.vars, other.vars),
            (&mut self.globals, other.globals),
        ] {
            for (name, ty) in source {
                dest.entry(name)
                    .and_modify(|old| {
                        *old = crate::blade::call_site_inference::join_call_site_types(vec![
                            old.clone(),
                            ty.clone(),
                        ]);
                    })
                    .or_insert(ty);
            }
        }
    }

    /// Generic views expose data through the renderer's property proxy.
    pub(super) fn this_type(&self) -> Option<PhpType> {
        if let Some(ty) = &self.bound_this {
            return Some(ty.clone());
        }
        (!self.vars.is_empty()).then(|| {
            PhpType::object_shape(
                self.vars
                    .iter()
                    .map(|(name, ty)| ShapeEntry {
                        key: Some(name.clone()),
                        value_type: ty.clone(),
                        optional: false,
                    })
                    .collect(),
            )
        })
    }
}

/// Inference inputs kept between refreshes, behind the refresh lock.
#[derive(Default)]
pub(crate) struct Memo {
    /// View implementations as of the last PHP change; `None` until the
    /// first refresh, which is then a full one.
    custom: Option<Arc<BTreeMap<String, super::custom_views::CustomView>>>,
    /// Data each render caller supplied, by caller URI.
    callers: BTreeMap<String, Caller>,
    calls: super::component_context::CallMemo,
}

struct Caller {
    targets: Vec<(String, Scope)>,
    /// Custom View classes this caller instantiates with its own data.
    instantiated: HashSet<String>,
}

/// Whether a file's symbol map can supply data to a view: a literal view
/// path, a `data()` call, or a `GenericView` reference.
fn renders_views(map: &SymbolMap) -> bool {
    map.spans.iter().any(|span| match &span.kind {
        SymbolKind::StringKey {
            kind: StringKeyKind::TempestView { .. },
            ..
        } => true,
        SymbolKind::MemberAccess {
            member_name,
            is_method_call: true,
            ..
        } => member_name.eq_ignore_ascii_case("data"),
        SymbolKind::ClassReference { name, .. } => name
            .trim_start_matches('\\')
            .eq_ignore_ascii_case("Tempest\\View\\GenericView"),
        _ => false,
    })
}

impl Backend {
    /// Whether this is a Tempest project with at least one view lowered.
    fn has_tempest_views(&self) -> bool {
        self.is_tempest_project()
            && self
                .tempest
                .has_views
                .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Whether a change to `uri` can change the data inferred for a view:
    /// it is a view, the last refresh read data from it, or it now renders
    /// one.  Edits to any other file leave every inferred scope as it was,
    /// so they skip the project-wide refresh.  A save refreshes regardless.
    pub(crate) fn affects_tempest_inference(&self, uri: &str) -> bool {
        self.has_tempest_views()
            && (super::is_view(uri)
                || self.tempest.sources.read().contains(uri)
                || self
                    .symbol_map_for(uri)
                    .is_some_and(|map| renders_views(&map)))
    }

    /// Recompute view inference from scratch: after a save, a watcher batch
    /// or an index pass, any declaration a view reads may have changed.
    /// Never called while holding the parser's publication locks.
    pub(crate) fn refresh_tempest_inference(&self) {
        self.refresh_inference(None);
    }

    /// Refresh after an unsaved edit to `uri`, one that
    /// [`Self::affects_tempest_inference`] accepted.  Only that file changed,
    /// so every other render caller's data and every component call whose
    /// templates and incoming scope are unchanged is reused.
    pub(crate) fn refresh_tempest_inference_after_edit(&self, uri: &str) {
        self.refresh_inference(Some(uri));
    }

    fn refresh_inference(&self, edited: Option<&str>) {
        if !self.has_tempest_views() {
            return;
        }
        let templates: Vec<_> = self
            .files
            .read()
            .keys()
            .filter(|uri| super::is_view(uri))
            .cloned()
            .collect();
        let mut memo = self.tempest.refresh.lock();
        let indexed: HashSet<_> = templates.iter().map(String::as_str).collect();
        self.tempest
            .scopes
            .write()
            .retain(|uri, _| indexed.contains(uri.as_str()));
        if templates.is_empty() {
            return;
        }
        let memo = &mut *memo;
        let edited = edited.filter(|_| memo.custom.is_some());
        if edited.is_none() {
            *memo = Memo::default();
        }
        let custom = match (&memo.custom, edited) {
            (Some(custom), Some(uri)) if !self.may_declare_custom_view(uri, custom) => {
                Arc::clone(custom)
            }
            _ => Arc::new(super::custom_views::collect(self)),
        };
        let rescan = memo.custom.as_ref() != Some(&custom);
        memo.custom = Some(Arc::clone(&custom));
        if rescan {
            memo.callers.clear();
            let callers: Vec<_> = self
                .files
                .read()
                .iter()
                .filter(|(_, file)| file.symbol_map.as_deref().is_some_and(renders_views))
                .map(|(uri, _)| uri.clone())
                .collect();
            for uri in callers {
                self.collect_caller(&uri, &custom, &mut memo.callers);
            }
        } else if let Some(uri) = edited {
            self.collect_caller(uri, &custom, &mut memo.callers);
        }
        let mut sources: HashSet<String> = memo.callers.keys().cloned().collect();
        sources.extend(custom.values().map(|view| view.source.clone()));
        *self.tempest.sources.write() = sources;
        let mut scopes: HashMap<String, Scope> = HashMap::new();
        let mut instantiated = HashSet::new();
        for caller in memo.callers.values() {
            instantiated.extend(caller.instantiated.iter().cloned());
            for (target, scope) in &caller.targets {
                scopes
                    .entry(target.clone())
                    .or_default()
                    .merge(scope.clone());
            }
        }
        for (name, view) in custom.iter() {
            if !instantiated.contains(name) {
                scopes
                    .entry(view.target.clone())
                    .or_default()
                    .merge(view.scope.clone());
            }
        }
        let scopes =
            super::component_context::infer(self, &templates, scopes, &mut memo.calls, edited);
        for uri in templates {
            let canonical = memo.calls.canonical.get(&uri);
            let fresh = scopes.get(&canonical).cloned().unwrap_or_default();
            self.publish_tempest_scope(&uri, &fresh);
        }
    }

    /// Whether an edit to `uri` can change the custom View classes: it
    /// declares one, or a class that could extend or implement one.  A view
    /// declares no classes.
    fn may_declare_custom_view(
        &self,
        uri: &str,
        custom: &BTreeMap<String, super::custom_views::CustomView>,
    ) -> bool {
        !super::is_view(uri)
            && (custom.values().any(|view| view.source == uri)
                || self
                    .symbols
                    .uri_classes_index
                    .read()
                    .get(uri)
                    .is_some_and(|classes| {
                        classes.iter().any(|class| {
                            !class.interfaces.is_empty() || class.parent_class.is_some()
                        })
                    }))
    }

    /// Walk one file's render calls into `callers`, or drop it when it no
    /// longer renders a view.  A file whose symbol map lags behind its text
    /// supplies no data until it parses, but stays a caller so its next
    /// edit refreshes.
    fn collect_caller(
        &self,
        uri: &str,
        custom: &BTreeMap<String, super::custom_views::CustomView>,
        callers: &mut BTreeMap<String, Caller>,
    ) {
        let Some(map) = self.symbol_map_for(uri).filter(|map| renders_views(map)) else {
            callers.remove(uri);
            return;
        };
        let mut caller = Caller {
            targets: Vec::new(),
            instantiated: HashSet::new(),
        };
        if let Some(content) = self.analysable_content(uri)
            && map.matches_source(&content)
            && (map.source_hash.is_none() || map.built_from(&content))
        {
            let file = self.file_context(uri);
            caller.targets = super::view_data::collect(
                self,
                uri,
                &file,
                &content,
                custom,
                &mut caller.instantiated,
            );
        }
        callers.insert(uri.to_owned(), caller);
    }

    /// Reparse only when inferred declarations or slot scopes changed.
    ///
    /// The view is reparsed from its current content under its `did_change`
    /// parse lock: a refresh can run for as long as a keystroke takes to
    /// arrive, and parsing the text it started from would publish an older
    /// buffer over the one that keystroke's parse published.  When the text
    /// has moved on, that keystroke's own refresh follows and corrects the
    /// scope if the edit changed it.
    pub(super) fn publish_tempest_scope(&self, uri: &str, fresh: &Scope) {
        let unchanged = self
            .tempest
            .scopes
            .read()
            .get(uri)
            .map_or_else(|| *fresh == Scope::default(), |old| **old == *fresh);
        if unchanged {
            return;
        }
        self.tempest
            .scopes
            .write()
            .insert(uri.to_owned(), Arc::new(fresh.clone()));
        let parse_lock = self.open_files.read().contains_key(uri).then(|| {
            Arc::clone(
                self.did_change_parse_locks
                    .lock()
                    .entry(uri.to_owned())
                    .or_default(),
            )
        });
        let _parse_guard = parse_lock.as_ref().map(|lock| lock.lock());
        let Some(content) = self.get_file_content_arc(uri) else {
            return;
        };
        self.update_ast(uri, &content);
        self.schedule_diagnostics(uri.to_owned());
    }
}

/// Names PHP can extract, excluding its non-rebindable `$this` variable.
pub(super) fn variable_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c >= 128)
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_' || c >= 128)
        && name != "this"
}
