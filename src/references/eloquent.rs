//! References to an Eloquent model method under the name the model uses it
//! by.
//!
//! `scopeActive()` is called as `active()`, `getFullNameAttribute()` is
//! read as `->full_name`, and so on (see
//! [`MagicMemberKind`](crate::virtual_members::laravel::MagicMemberKind)).
//! A search from the declaration therefore also looks for accesses to the
//! magic name whose receiver is the model.  A scope is also reached through
//! the model's query builder and its relations, whose class alone does not
//! say which model they query (`Builder<BlogAuthor>` and `Builder<Post>`
//! are the same class), so for those the receiver's model argument decides.

use super::*;

use std::cell::OnceCell;

use crate::atom::{Atom, AtomMap};
use crate::class_lookup::find_class_at_offset;
use crate::php_type::{PhpType, TypeKind};
use crate::references::member_scope::MemberScope;
use crate::symbol_map::SymbolKind;
use crate::types::FileContext;
use crate::virtual_members::laravel::{
    ELOQUENT_BUILDER_FQN, MagicMemberKind, extends_eloquent_model,
};

const RELATION_FQN: &str = "Illuminate\\Database\\Eloquent\\Relations\\Relation";

/// A model method that is used under a magic name.
pub(crate) struct EloquentMagicMember {
    /// The model that declares the method.
    model: String,
    pub(crate) kind: MagicMemberKind,
    /// The name the model's users spell (`active`, `full_name`).
    pub(crate) use_name: Atom,
}

impl Backend {
    /// The magic use of the method `member_name` declared at `offset`, or
    /// `None` when it is not an Eloquent model method used under a magic
    /// name.
    pub(crate) fn eloquent_magic_member_at(
        &self,
        uri: &str,
        offset: u32,
        member_name: &str,
    ) -> Option<EloquentMagicMember> {
        let classes = self.symbols.uri_classes_index.read().get(uri).cloned()?;
        let class = find_class_at_offset(&classes, offset)?;
        let method = class.get_method(member_name)?;
        let kind = MagicMemberKind::of(method)?;
        let use_name = kind.use_name(&method.name)?;
        let class_loader = |name: &str| self.find_or_load_class(name);
        if !extends_eloquent_model(class, &class_loader) {
            return None;
        }
        Some(EloquentMagicMember {
            model: normalize_fqn(&class.fqn()),
            kind,
            use_name: crate::atom::atom(&use_name),
        })
    }

    /// The accesses to each member's magic name, in one pass over the union
    /// of their candidate files, optionally narrowed to `restrict_to`.
    pub(crate) fn eloquent_magic_references_batch(
        &self,
        members: &[&EloquentMagicMember],
        restrict_to: Option<&HashSet<Arc<str>>>,
    ) -> Vec<Vec<Location>> {
        struct Prepared {
            is_method: bool,
            /// The receivers that are the model itself: the model, its
            /// subclasses, and for a scope its custom query builder.
            scope: MemberScope,
        }

        if members.is_empty() {
            return Vec::new();
        }

        let mut by_name: AtomMap<Vec<usize>> = AtomMap::default();
        let mut candidate_keys = HashSet::new();
        let prepared: Vec<Prepared> = members
            .iter()
            .enumerate()
            .map(|(index, member)| {
                by_name.entry(member.use_name).or_default().push(index);
                // A scope is called both statically on the model and on a
                // builder instance.
                for is_static in [false, true] {
                    candidate_keys.insert(ReferenceIndexKey::Member {
                        name: member.use_name.to_string(),
                        is_static,
                    });
                }
                let mut roots = HashSet::from([member.model.clone()]);
                if member.kind.is_method()
                    && let Some(builder) = self.find_or_load_class(&member.model).and_then(|c| {
                        c.laravel()?
                            .custom_builder
                            .as_ref()?
                            .base_name()
                            .map(normalize_fqn)
                    })
                {
                    roots.insert(builder);
                }
                Prepared {
                    is_method: member.kind.is_method(),
                    scope: self.descendant_scope(roots),
                }
            })
            .collect();
        let forwarding = self.descendant_scope(HashSet::from([
            ELOQUENT_BUILDER_FQN.to_string(),
            RELATION_FQN.to_string(),
        ]));

        let candidate_keys: Vec<_> = candidate_keys.into_iter().collect();
        let mut snapshot = self.user_file_symbol_maps_for_reference_keys(&candidate_keys);
        if let Some(files) = restrict_to {
            snapshot.retain(|(uri, _)| files.contains(uri.as_str()));
        }

        let scan_file = |file: &CandidateFile<'_>,
                         symbol_map: &Arc<SymbolMap>|
         -> Vec<(usize, Location)> {
            let names: Vec<Atom> = by_name
                .keys()
                .copied()
                .filter(|name| !symbol_map.member_access_indices(name).is_empty())
                .collect();
            if names.is_empty() {
                return Vec::new();
            }
            let Some(resolved) = self.member_receivers_for(file, symbol_map, &names) else {
                return Vec::new();
            };
            let Some(uri) = file.url() else {
                return Vec::new();
            };
            let file_ctx = OnceCell::new();

            let mut matches = Vec::new();
            for name in &names {
                for &span_index in symbol_map.member_access_indices(name) {
                    let SymbolKind::MemberAccess { is_method_call, .. } =
                        &symbol_map.spans[span_index].kind
                    else {
                        continue;
                    };
                    let Some((range, targets)) = resolved.resolved_access(span_index) else {
                        continue;
                    };
                    let forwarded_models = OnceCell::new();
                    for &index in &by_name[name] {
                        let query = &prepared[index];
                        if query.is_method != *is_method_call {
                            continue;
                        }
                        let on_model = targets.iter().any(|fqn| query.scope.contains(self, fqn));
                        let through_builder = || {
                            query.is_method
                                && targets.iter().any(|fqn| forwarding.contains(self, fqn))
                                && forwarded_models
                                    .get_or_init(|| {
                                        self.forwarded_model_fqns(
                                            file, symbol_map, span_index, &file_ctx,
                                        )
                                    })
                                    .iter()
                                    .any(|fqn| query.scope.contains(self, fqn))
                        };
                        if on_model || through_builder() {
                            matches.push((
                                index,
                                Location {
                                    uri: uri.clone(),
                                    range,
                                },
                            ));
                        }
                    }
                }
            }
            matches
        };

        let mut locations = vec![Vec::new(); members.len()];
        for (index, location) in self.scan_candidate_snapshot(
            snapshot,
            "Scanning for model member references",
            scan_file,
        ) {
            locations[index].push(location);
        }
        for member_locations in &mut locations {
            sort_locations_for_references(member_locations);
        }
        locations
    }

    /// The models a query builder or relation receiver queries: the first
    /// type argument of `Builder<TModel>` and of `HasMany<TRelated, …>`.
    ///
    /// The recorded receiver of an access is its class alone, so the full
    /// type is resolved again here.  Only the accesses whose receiver is a
    /// builder or a relation ask, which keeps this off the common path.
    fn forwarded_model_fqns(
        &self,
        file: &CandidateFile<'_>,
        symbol_map: &Arc<SymbolMap>,
        span_index: usize,
        file_ctx: &OnceCell<FileContext>,
    ) -> Vec<String> {
        let span = &symbol_map.spans[span_index];
        let SymbolKind::MemberAccess {
            subject_text,
            is_static,
            ..
        } = &span.kind
        else {
            return Vec::new();
        };
        let Some(content) = file.content() else {
            return Vec::new();
        };
        let Some(source) = symbol_map.source(content) else {
            return Vec::new();
        };
        let ctx = file_ctx.get_or_init(|| self.file_context(file.uri()));

        // The same activations `resolve_member_receivers` makes for the
        // walk that recorded the receiver: a scan worker has no request
        // caches of its own.
        let _resolved_classes_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);
        let _parse_cache_guard = crate::parser::with_parse_cache_arc(Arc::clone(content));
        let _chain_guard = crate::type_engine::resolver::with_isolated_chain_cache();
        let Some(subject_type) = self.resolve_subject_type_at(
            subject_text.as_str(source),
            *is_static,
            ctx,
            span.start,
            content,
        ) else {
            return Vec::new();
        };
        let mut names = Vec::new();
        collect_first_type_arguments(&subject_type, &mut names);
        self.class_names_to_fqns(names, ctx, span.start)
    }
}

/// The class names in the first type argument of each generic member of
/// `ty`.
fn collect_first_type_arguments(ty: &PhpType, names: &mut Vec<String>) {
    match ty.kind() {
        TypeKind::Union(types) | TypeKind::Intersection(types) => {
            for member in types {
                collect_first_type_arguments(member, names);
            }
        }
        TypeKind::Nullable(inner) => collect_first_type_arguments(inner, names),
        TypeKind::Generic(generic) => {
            if let Some(first) = generic.args.first() {
                names.extend(first.top_level_class_names());
            }
        }
        _ => {}
    }
}
