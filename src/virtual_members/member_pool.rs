//! Content-keyed pool of resolved-class members.
//!
//! The inheritance and virtual-member merges rebuild members whose
//! content is identical across many classes: a trait method with a bare
//! `self` re-merged for every subclass of the class that uses the trait,
//! `offsetExists(TKey)` substituted once per `Collection<int, X>`, an
//! interface enrichment copied on write per implementing class.  The
//! transform interning in [`super::cache`] only collapses copies whose
//! transform fingerprints match, so copies reached through different
//! paths (or fingerprinted by more than the method actually names) stay
//! separate allocations.
//!
//! When a resolved class enters the cache, each of its methods,
//! properties and parameter lists is looked up here by content and
//! replaced with the existing `Arc` when one is live.  The tables hold
//! [`Weak`] references, so a member lives exactly as long as some class
//! refers to it, as in the [`PhpType`](crate::php_type::PhpType)
//! interner.  Member types are interned `PhpType`s, which hash and
//! compare as pointers, so hashing a member is cheap.
//!
//! Each table keeps one entry per 64-bit content hash.  A hash collision
//! between different contents only costs a missed share: lookups always
//! confirm full equality before handing out an `Arc`.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::sync::{Arc, Weak};

use crate::php_type::{FxHasher, PassThroughHasher};
use crate::types::{ClassInfo, MethodInfo, ParameterInfo, PropertyInfo, SharedVec};

type Table<T> = HashMap<u64, Weak<T>, BuildHasherDefault<PassThroughHasher>>;

/// Minimum number of insertions before the tables are swept for entries
/// whose last strong reference has been dropped.
const SWEEP_INTERVAL: usize = 4096;

#[derive(Default)]
pub(crate) struct MemberPool {
    methods: Table<MethodInfo>,
    properties: Table<PropertyInfo>,
    parameters: Table<Vec<ParameterInfo>>,
    /// Insertions since the last sweep.  A dead entry still pins its
    /// allocation (the `Arc` header and the struct itself) until the
    /// `Weak` is dropped, so the tables are swept periodically.
    since_sweep: usize,
}

impl MemberPool {
    /// Replace every method, property and parameter list of `class` with
    /// the pooled `Arc` holding the same content, and pool the ones not
    /// seen before.
    pub(crate) fn share_class(&mut self, class: &mut ClassInfo) {
        self.share_methods(&mut class.methods);
        self.share_properties(&mut class.properties);
        self.maybe_sweep();
    }

    /// Return the pooled method equal to `method`, pooling it when none
    /// is live.
    pub(crate) fn share_method(&mut self, mut method: MethodInfo) -> Arc<MethodInfo> {
        let hash = method_hash(&method);
        if let Some(pooled) = find(&self.methods, hash, |m| method_eq(m, &method)) {
            return pooled;
        }
        self.share_parameters(&mut method.parameters);
        let method = Arc::new(method);
        self.since_sweep += record(&mut self.methods, hash, &method);
        self.maybe_sweep();
        method
    }

    /// Return the pooled property equal to `property`, pooling it when
    /// none is live.
    pub(crate) fn share_property(&mut self, property: PropertyInfo) -> Arc<PropertyInfo> {
        let hash = property_hash(&property);
        if let Some(pooled) = find(&self.properties, hash, |p| property_eq(p, &property)) {
            return pooled;
        }
        let property = Arc::new(property);
        self.since_sweep += record(&mut self.properties, hash, &property);
        self.maybe_sweep();
        property
    }

    pub(crate) fn clear(&mut self) {
        self.methods.clear();
        self.properties.clear();
        self.parameters.clear();
        self.since_sweep = 0;
    }

    fn share_methods(&mut self, methods: &mut SharedVec<Arc<MethodInfo>>) {
        // Decide every swap before touching the vector, so a class whose
        // members are all pooled already never copies a vector it shares
        // with another class.
        let mut swaps: Vec<(usize, Arc<MethodInfo>)> = Vec::new();
        let mut new_params: Vec<(usize, u64, Arc<Vec<ParameterInfo>>)> = Vec::new();
        let mut fresh: Vec<(usize, u64)> = Vec::new();
        // Swapping the parameter list of a method nobody else holds is
        // done in place.  A method held elsewhere keeps its own list:
        // rebuilding it would cost more than the list saves.
        let methods_unique = methods.is_unique();

        for (i, method) in methods.iter().enumerate() {
            let hash = method_hash(method);
            if let Some(pooled) = find(&self.methods, hash, |m| {
                std::ptr::eq(m, &**method) || method_eq(m, method)
            }) {
                if !Arc::ptr_eq(&pooled, method) {
                    swaps.push((i, pooled));
                }
                continue;
            }
            match self.pooled_parameters(&method.parameters) {
                Some(pooled)
                    if methods_unique
                        && Arc::strong_count(method) == 1
                        && Arc::weak_count(method) == 0 =>
                {
                    new_params.push((i, hash, pooled));
                }
                _ => fresh.push((i, hash)),
            }
        }

        if !swaps.is_empty() || !new_params.is_empty() {
            let methods = methods.make_mut();
            for (i, pooled) in swaps {
                methods[i] = pooled;
            }
            for (i, hash, params) in new_params {
                if let Some(method) = Arc::get_mut(&mut methods[i]) {
                    method.parameters = SharedVec::from_arc(params);
                }
                fresh.push((i, hash));
            }
        }
        for (i, hash) in fresh {
            self.since_sweep += record(&mut self.methods, hash, &methods[i]);
        }
    }

    fn share_properties(&mut self, properties: &mut SharedVec<Arc<PropertyInfo>>) {
        let mut swaps: Vec<(usize, Arc<PropertyInfo>)> = Vec::new();
        for (i, property) in properties.iter().enumerate() {
            let hash = property_hash(property);
            match find(&self.properties, hash, |p| {
                std::ptr::eq(p, &**property) || property_eq(p, property)
            }) {
                Some(pooled) if !Arc::ptr_eq(&pooled, property) => swaps.push((i, pooled)),
                Some(_) => {}
                None => self.since_sweep += record(&mut self.properties, hash, property),
            }
        }
        if !swaps.is_empty() {
            let properties = properties.make_mut();
            for (i, pooled) in swaps {
                properties[i] = pooled;
            }
        }
    }

    /// The pooled list equal to `params` when it is a different
    /// allocation, pooling `params` itself when no equal list is live.
    fn pooled_parameters(
        &mut self,
        params: &SharedVec<ParameterInfo>,
    ) -> Option<Arc<Vec<ParameterInfo>>> {
        let own = params.as_arc();
        let hash = parameters_hash(params);
        match find(&self.parameters, hash, |p| {
            std::ptr::eq(p, &**own) || parameters_eq(p, params)
        }) {
            Some(pooled) if !Arc::ptr_eq(&pooled, own) => Some(pooled),
            Some(_) => None,
            None => {
                self.since_sweep += record(&mut self.parameters, hash, own);
                None
            }
        }
    }

    fn share_parameters(&mut self, params: &mut SharedVec<ParameterInfo>) {
        if let Some(pooled) = self.pooled_parameters(params) {
            *params = SharedVec::from_arc(pooled);
        }
    }

    fn maybe_sweep(&mut self) {
        let len = self.methods.len() + self.properties.len() + self.parameters.len();
        if self.since_sweep < SWEEP_INTERVAL.max(len) {
            return;
        }
        self.since_sweep = 0;
        self.methods.retain(|_, w| w.strong_count() > 0);
        self.properties.retain(|_, w| w.strong_count() > 0);
        self.parameters.retain(|_, w| w.strong_count() > 0);
    }
}

fn find<T>(table: &Table<T>, hash: u64, eq: impl FnOnce(&T) -> bool) -> Option<Arc<T>> {
    let live = table.get(&hash)?.upgrade()?;
    eq(&live).then_some(live)
}

/// Pool `value` under `hash` unless a different live value already holds
/// the slot.  Returns the number of insertions made (0 or 1).
fn record<T>(table: &mut Table<T>, hash: u64, value: &Arc<T>) -> usize {
    match table.entry(hash) {
        Entry::Vacant(slot) => {
            slot.insert(Arc::downgrade(value));
            1
        }
        Entry::Occupied(mut slot) if slot.get().strong_count() == 0 => {
            slot.insert(Arc::downgrade(value));
            1
        }
        Entry::Occupied(_) => 0,
    }
}

// The hashes cover the fields most likely to tell two members apart; the
// equality checks below cover every field, destructured so that adding a
// field to a member struct fails to compile here until it is compared.

fn method_hash(m: &MethodInfo) -> u64 {
    let mut h = FxHasher::default();
    m.name.hash(&mut h);
    m.name_offset.hash(&mut h);
    m.return_type.hash(&mut h);
    m.is_static.hash(&mut h);
    m.is_virtual.hash(&mut h);
    (m.visibility as u8).hash(&mut h);
    m.description.as_ref().map(String::len).hash(&mut h);
    parameters_hash_into(&m.parameters, &mut h);
    h.finish()
}

fn property_hash(p: &PropertyInfo) -> u64 {
    let mut h = FxHasher::default();
    p.name.hash(&mut h);
    p.name_offset.hash(&mut h);
    p.type_hint.hash(&mut h);
    p.is_static.hash(&mut h);
    p.is_virtual.hash(&mut h);
    (p.visibility as u8).hash(&mut h);
    p.description.as_ref().map(String::len).hash(&mut h);
    h.finish()
}

fn parameters_hash(params: &[ParameterInfo]) -> u64 {
    let mut h = FxHasher::default();
    parameters_hash_into(params, &mut h);
    h.finish()
}

fn parameters_hash_into(params: &[ParameterInfo], h: &mut FxHasher) {
    params.len().hash(h);
    for p in params {
        p.name.hash(h);
        p.type_hint.hash(h);
        p.default_value.hash(h);
    }
}

fn method_eq(a: &MethodInfo, b: &MethodInfo) -> bool {
    let MethodInfo {
        name,
        name_offset,
        parameters,
        return_type,
        native_return_type,
        description,
        is_static,
        visibility,
        has_scope_attribute,
        is_abstract,
        is_final,
        is_virtual,
        is_macro,
        is_inferred_return,
        is_pure,
        is_impure,
        reads_func_args,
        is_no_discard,
        extras: _,
    } = a;
    *name == b.name
        && *name_offset == b.name_offset
        && *return_type == b.return_type
        && *is_static == b.is_static
        && *visibility == b.visibility
        && *is_virtual == b.is_virtual
        && *native_return_type == b.native_return_type
        && *has_scope_attribute == b.has_scope_attribute
        && *is_abstract == b.is_abstract
        && *is_final == b.is_final
        && *is_macro == b.is_macro
        && *is_inferred_return == b.is_inferred_return
        && *is_pure == b.is_pure
        && *is_impure == b.is_impure
        && *reads_func_args == b.reads_func_args
        && *is_no_discard == b.is_no_discard
        && *description == b.description
        && a.extras_eq(b)
        && parameters_eq(parameters, &b.parameters)
}

fn property_eq(a: &PropertyInfo, b: &PropertyInfo) -> bool {
    let PropertyInfo {
        name,
        name_offset,
        type_hint,
        native_type_hint,
        description,
        is_static,
        is_readonly,
        visibility,
        deprecation_message,
        deprecated_replacement,
        see_refs,
        is_virtual,
        source,
    } = a;
    *name == b.name
        && *name_offset == b.name_offset
        && *type_hint == b.type_hint
        && *native_type_hint == b.native_type_hint
        && *is_static == b.is_static
        && *is_readonly == b.is_readonly
        && *visibility == b.visibility
        && *is_virtual == b.is_virtual
        && *description == b.description
        && *deprecation_message == b.deprecation_message
        && *deprecated_replacement == b.deprecated_replacement
        && *see_refs == b.see_refs
        && *source == b.source
}

fn parameters_eq(a: &[ParameterInfo], b: &[ParameterInfo]) -> bool {
    std::ptr::eq(a, b) || (a.len() == b.len() && a.iter().zip(b).all(|(a, b)| parameter_eq(a, b)))
}

fn parameter_eq(a: &ParameterInfo, b: &ParameterInfo) -> bool {
    let ParameterInfo {
        name,
        is_required,
        type_hint,
        native_type_hint,
        description,
        default_value,
        is_variadic,
        is_reference,
        closure_this_type,
        param_out_type,
        callable_invocation,
    } = a;
    *name == b.name
        && *is_required == b.is_required
        && *type_hint == b.type_hint
        && *native_type_hint == b.native_type_hint
        && *is_variadic == b.is_variadic
        && *is_reference == b.is_reference
        && *closure_this_type == b.closure_this_type
        && *param_out_type == b.param_out_type
        && *callable_invocation == b.callable_invocation
        && *default_value == b.default_value
        && *description == b.description
}
