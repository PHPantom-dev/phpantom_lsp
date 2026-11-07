//! Data passed to Tempest views, resolved in each caller's PHP scope.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::Backend;
use crate::php_type::{PhpType, ShapeEntry};
use crate::symbol_map::{StringKeyKind, SymbolKind};

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

impl Backend {
    /// Refresh after a published edit or an index pass, never while holding
    /// the parser's publication locks. Each caller is walked once for all
    /// the templates it renders.
    pub(crate) fn refresh_tempest_inference(&self) {
        if !self
            .tempest
            .has_views
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let templates: Vec<_> = self
            .files
            .read()
            .keys()
            .filter(|uri| super::is_view(uri))
            .cloned()
            .collect();
        let _refresh = self.tempest.refresh.lock();
        let indexed: std::collections::HashSet<_> = templates.iter().map(String::as_str).collect();
        self.tempest
            .scopes
            .write()
            .retain(|uri, _| indexed.contains(uri.as_str()));
        if templates.is_empty() {
            return;
        }
        let custom = super::custom_views::collect(self);
        let callers: Vec<_> = self
            .files
            .read()
            .iter()
            .filter_map(|(uri, file)| {
                let map = file.symbol_map.as_ref()?;
                map.spans
                    .iter()
                    .any(|span| matches!(span.kind, SymbolKind::StringKey { kind: StringKeyKind::TempestView { .. }, .. }) || matches!(&span.kind, SymbolKind::MemberAccess { member_name, is_method_call: true, .. } if member_name.eq_ignore_ascii_case("data"))
                        || matches!(&span.kind, SymbolKind::ClassReference { name, .. } if name.trim_start_matches('\\').eq_ignore_ascii_case("Tempest\\View\\GenericView")))
                    .then(|| (uri.clone(), Arc::clone(map)))
            })
            .collect();
        let mut scopes: HashMap<String, Scope> = HashMap::new();
        let mut instantiated = std::collections::HashSet::new();
        for (uri, map) in callers {
            let Some(content) = self.analysable_content(&uri) else {
                continue;
            };
            if !map.matches_source(&content)
                || (map.source_hash.is_some() && !map.built_from(&content))
            {
                continue;
            }
            let file = self.file_context(&uri);
            for (target, scope) in
                super::view_data::collect(self, &uri, &file, &content, &custom, &mut instantiated)
            {
                scopes.entry(target).or_default().merge(scope);
            }
        }
        for (name, view) in &custom {
            if !instantiated.contains(name) {
                scopes
                    .entry(view.target.clone())
                    .or_default()
                    .merge(view.scope.clone());
            }
        }
        let scopes = super::component_context::infer(self, &templates, scopes);
        for uri in templates {
            let canonical = super::discovery::canonical_uri(&uri);
            let fresh = scopes.get(&canonical).cloned().unwrap_or_default();
            if let Some(content) = self.get_file_content_arc(&uri) {
                self.publish_tempest_scope(&uri, &content, &fresh);
            }
        }
    }

    /// Reparse only when inferred declarations or slot scopes changed.
    pub(super) fn publish_tempest_scope(&self, uri: &str, content: &str, fresh: &Scope) {
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
        self.update_ast(uri, content);
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
