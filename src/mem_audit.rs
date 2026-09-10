//! Temporary deep-size audit of the long-lived symbol stores.
//!
//! Investigation tooling for the workspace-memory work: walks the
//! resolved-class cache and the persistent indexes with pointer
//! dedup (shared `Arc`s counted once) and attributes live heap bytes
//! to field-level categories, so representation changes can be ranked
//! by exactly how much they would reclaim.
//!
//! Gated behind the `PHPANTOM_MEM_AUDIT` environment variable in the
//! `analyze` pipeline. Not shipped behaviour; remove after the memory
//! investigation concludes.

// The helpers deliberately take `&Vec<_>` so they can read `capacity()`.
#![allow(clippy::ptr_arg)]

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use crate::Backend;
use crate::atom::{Atom, AtomMap};
use crate::php_type::{
    CallableParam, CallableType, ConditionalType, GenericType, LiteralValue, PhpType, ShapeEntry,
    TypeKind,
};
use crate::types::{
    ClassInfo, ConstantInfo, DocblockMembers, FunctionInfo, LaravelMetadata, MethodInfo,
    ParameterInfo, PropertyInfo, PropertySource, SharedVec, TraitAlias, TraitPrecedence,
    TypeAliasDef, TypeAssertion,
};

// ─── Counting global allocator ──────────────────────────────────────────────

/// Bytes currently live (sum of requested layout sizes).
static LIVE_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Allocations currently live.
static LIVE_ALLOCS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The allocator this build would use if the audit were not enabled, so
/// the numbers describe the allocator that actually ships.
#[cfg(all(feature = "mimalloc", target_os = "linux"))]
const INNER: mimalloc::MiMalloc = mimalloc::MiMalloc;
#[cfg(not(all(feature = "mimalloc", target_os = "linux")))]
const INNER: std::alloc::System = std::alloc::System;

/// Name of the delegated allocator, for labelling audit output.
pub(crate) const INNER_NAME: &str = if cfg!(all(feature = "mimalloc", target_os = "linux")) {
    "mimalloc"
} else {
    "system"
};

/// Passthrough allocator that tracks live bytes and live allocation
/// count, so the audit can separate "bytes we asked for" from "resident
/// pages the allocator holds". Delegates to [`INNER`].
pub(crate) struct CountingAlloc;

unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { INNER.alloc(layout) };
        if !p.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
            LIVE_ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { INNER.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
        LIVE_ALLOCS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { INNER.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            LIVE_BYTES.fetch_add(new_size, std::sync::atomic::Ordering::Relaxed);
            LIVE_BYTES.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { INNER.alloc_zeroed(layout) };
        if !p.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
            LIVE_ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        p
    }
}

/// Live bytes / allocations as seen by [`CountingAlloc`].
fn live_heap() -> (usize, usize) {
    (
        LIVE_BYTES.load(std::sync::atomic::Ordering::Relaxed),
        LIVE_ALLOCS.load(std::sync::atomic::Ordering::Relaxed),
    )
}

/// Arc control block (strong + weak counts).
const ARC: usize = 16;
/// Vec header stored inside an `Arc<Vec<T>>` allocation.
const VEC_HDR: usize = 24;
const PT: usize = size_of::<PhpType>();

#[derive(Default, Clone, Copy)]
struct Sz {
    bytes: usize,
    allocs: usize,
    /// Count of `PhpType`-sized slots inside heap allocations (`Vec`
    /// elements, `Box` targets, map buckets). Multiplying this by the
    /// size a shrunk `PhpType` would save gives the reclaim for the
    /// heap side; inline struct fields are counted separately.
    slots: usize,
}

impl Sz {
    fn add(&mut self, bytes: usize) {
        if bytes > 0 {
            self.bytes += bytes;
            self.allocs += 1;
        }
    }

    fn slot(&mut self, n: usize) {
        self.slots += n;
    }
}

impl std::ops::AddAssign for Sz {
    fn add_assign(&mut self, o: Sz) {
        self.bytes += o.bytes;
        self.allocs += o.allocs;
        self.slots += o.slots;
    }
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Approximate hashbrown table allocation for a map with `cap` usable
/// capacity and `(K, V)` buckets: buckets are the next power of two
/// holding cap at 7/8 load, one ctrl byte per bucket.
fn map_buckets<K, V>(cap: usize) -> Sz {
    let mut z = Sz::default();
    if cap > 0 {
        let buckets = ((cap * 8).div_ceil(7).max(4)).next_power_of_two();
        z.add(buckets * (size_of::<(K, V)>() + 1));
    }
    z
}

fn s(o: &Option<String>) -> Sz {
    let mut z = Sz::default();
    if let Some(x) = o {
        z.add(x.capacity());
    }
    z
}

fn vs(v: &Vec<String>) -> Sz {
    let mut z = Sz::default();
    z.add(v.capacity() * size_of::<String>());
    for x in v {
        z.add(x.capacity());
    }
    z
}

fn ty_vec(v: &[PhpType]) -> Sz {
    let mut z = Sz::default();
    z.add(v.len() * PT);
    z.slot(v.len());
    for t in v {
        z += ty(t);
    }
    z
}

/// Per-variant counts plus distinct nodes, to size up which variants carry
/// the interner's payload and how much sharing the workload actually gets.
#[derive(Default)]
struct TypeStats {
    variants: HashMap<&'static str, usize>,
    /// Interned nodes reached at least once, by address. Types are
    /// hash-consed, so this is exactly the distinct-form count.
    distinct: HashSet<usize>,
    total: usize,
}

thread_local! {
    static TYPE_STATS: std::cell::RefCell<TypeStats> =
        std::cell::RefCell::new(TypeStats::default());
}

fn variant_name(t: &PhpType) -> &'static str {
    match t.kind() {
        TypeKind::Named(_) => "Named",
        TypeKind::StaticType(_) => "StaticType",
        TypeKind::ThisType(_) => "ThisType",
        TypeKind::Nullable(_) => "Nullable",
        TypeKind::Union(_) => "Union",
        TypeKind::Intersection(_) => "Intersection",
        TypeKind::Generic(..) => "Generic",
        TypeKind::Array(_) => "Array",
        TypeKind::ArrayShape(_) => "ArrayShape",
        TypeKind::ObjectShape(_) => "ObjectShape",
        TypeKind::Callable(_) => "Callable",
        TypeKind::Conditional(_) => "Conditional",
        TypeKind::ClassString(_) => "ClassString",
        TypeKind::InterfaceString(_) => "InterfaceString",
        TypeKind::KeyOf(_) => "KeyOf",
        TypeKind::ValueOf(_) => "ValueOf",
        TypeKind::IntRange(..) => "IntRange",
        TypeKind::IndexAccess(..) => "IndexAccess",
        TypeKind::Literal(_) => "Literal",
        TypeKind::Raw(_) => "Raw",
        TypeKind::Benevolent(_) => "Benevolent",
        TypeKind::ListShape(_) => "ListShape",
    }
}

/// Record one *occurrence* of a type, returning whether this is the first
/// time the audit has reached its interned node.
fn record_type(t: &PhpType) -> bool {
    TYPE_STATS.with(|s| {
        let mut s = s.borrow_mut();
        s.total += 1;
        *s.variants.entry(variant_name(t)).or_default() += 1;
        s.distinct.insert(std::ptr::from_ref(t.kind()) as usize)
    })
}

/// Heap bytes hanging off one `PhpType` occurrence.
///
/// Types are interned, so an occurrence costs one pointer-sized slot in
/// whatever holds it, and the node itself is charged once no matter how
/// many members share it — the same pointer-dedup rule the audit already
/// applies to shared `Arc` members.
fn ty(t: &PhpType) -> Sz {
    let mut z = Sz::default();
    if !record_type(t) {
        return z;
    }

    // The node's own `Arc` allocation: control block plus the enum.
    z.add(ARC + size_of::<TypeKind>());

    match t.kind() {
        TypeKind::Named(_) | TypeKind::StaticType(_) | TypeKind::ThisType(_) => {}
        TypeKind::Nullable(b)
        | TypeKind::Array(b)
        | TypeKind::KeyOf(b)
        | TypeKind::ValueOf(b)
        | TypeKind::Benevolent(b)
        | TypeKind::ListShape(b) => {
            z.slot(1);
            z += ty(b);
        }
        TypeKind::Union(v) | TypeKind::Intersection(v) => z += ty_vec(v),
        TypeKind::Generic(g) => {
            z.add(size_of::<GenericType>());
            z += ty_vec(&g.args);
        }
        TypeKind::ArrayShape(v) | TypeKind::ObjectShape(v) => {
            z.add(v.len() * size_of::<ShapeEntry>());
            z.slot(v.len());
            for e in v {
                if let Some(k) = &e.key {
                    z.add(k.capacity());
                }
                z += ty(&e.value_type);
            }
        }
        TypeKind::Callable(c) => {
            z.add(size_of::<CallableType>());
            // The boxed payload always holds one `Option<PhpType>` slot,
            // whether or not a return type was written.
            z.slot(1);
            z.add(c.params.capacity() * size_of::<CallableParam>());
            z.slot(c.params.capacity());
            for p in &c.params {
                z += ty(&p.type_hint);
            }
            if let Some(b) = &c.return_type {
                z += ty(b);
            }
        }
        TypeKind::Conditional(c) => {
            z.add(size_of::<ConditionalType>());
            z.slot(3);
            for b in [&c.condition, &c.then_type, &c.else_type] {
                z += ty(b);
            }
        }
        TypeKind::ClassString(o) | TypeKind::InterfaceString(o) => {
            if let Some(b) = o {
                z.slot(1);
                z += ty(b);
            }
        }
        // Interned bounds: no per-node heap payload.
        TypeKind::IntRange(..) => {}
        TypeKind::IndexAccess(a, b) => {
            z.slot(2);
            z += ty(a);
            z += ty(b);
        }
        TypeKind::Literal(l) => {
            z.add(size_of::<LiteralValue>());
            match &**l {
                LiteralValue::Int(x) | LiteralValue::Float(x) | LiteralValue::String(x) => {
                    z.add(x.len())
                }
            }
        }
        TypeKind::Raw(x) => z.add(x.len()),
    }
    z
}

fn opt_ty(o: &Option<PhpType>) -> Sz {
    match o {
        Some(t) => ty(t),
        None => Sz::default(),
    }
}

fn atom_map_ty(m: &AtomMap<PhpType>) -> Sz {
    let mut z = map_buckets::<Atom, PhpType>(m.capacity());
    if m.capacity() > 0 {
        z.slot(((m.capacity() * 8).div_ceil(7).max(4)).next_power_of_two());
    }
    for v in m.values() {
        z += ty(v);
    }
    z
}

fn assertions(v: &Vec<TypeAssertion>) -> Sz {
    let mut z = Sz::default();
    z.add(v.capacity() * size_of::<TypeAssertion>());
    z.slot(v.capacity());
    for a in v {
        z.add(a.param_name.capacity());
        z += ty(&a.asserted_type);
    }
    z
}

fn generic_pairs(v: &Vec<(Atom, Vec<PhpType>)>) -> Sz {
    let mut z = Sz::default();
    z.add(v.capacity() * size_of::<(Atom, Vec<PhpType>)>());
    for (_, args) in v {
        z += ty_vec(args);
    }
    z
}

fn property_source(src: &PropertySource) -> Sz {
    let mut z = Sz::default();
    let col = |z: &mut Sz, c: &crate::types::DatabaseColumnSource| {
        z.add(c.connection.capacity());
        z.add(c.table.capacity());
        z.add(c.column.capacity());
        z.add(c.database_type.capacity());
        if let Some(d) = &c.default {
            z.add(d.capacity());
        }
        if let Some(g) = &c.generated_expression {
            z.add(g.capacity());
        }
        if let Some(g) = &c.generated_mode {
            z.add(g.capacity());
        }
    };
    match src {
        PropertySource::DeclaredDefault { value } => z.add(value.len()),
        PropertySource::DatabaseColumn {
            column,
            attribute_default,
            mutator,
        } => {
            col(&mut z, column);
            if let Some(a) = attribute_default {
                z.add(a.value.capacity());
            }
            if let Some(m) = mutator {
                z.add(m.capacity());
            }
        }
        PropertySource::Cast {
            cast,
            column,
            attribute_default,
            mutator,
        } => {
            z.add(cast.capacity());
            if let Some(c) = column {
                col(&mut z, c);
            }
            if let Some(a) = attribute_default {
                z.add(a.value.capacity());
            }
            if let Some(m) = mutator {
                z.add(m.capacity());
            }
        }
        PropertySource::Accessor {
            method,
            mutator,
            column,
        } => {
            z.add(method.capacity());
            if let Some(m) = mutator {
                z.add(m.capacity());
            }
            if let Some(c) = column {
                col(&mut z, c);
            }
        }
        PropertySource::AttributeDefault {
            default,
            column,
            mutator,
        } => {
            z.add(default.value.capacity());
            if let Some(c) = column {
                col(&mut z, c);
            }
            if let Some(m) = mutator {
                z.add(m.capacity());
            }
        }
        PropertySource::ComputedProperty { method, mutator } => {
            z.add(method.capacity());
            if let Some(m) = mutator {
                z.add(m.capacity());
            }
        }
        PropertySource::Relationship {
            method,
            kind,
            pivot_accessor: _,
            pivot_using,
            pivot_columns,
        } => {
            z.add(method.capacity());
            z.add(kind.capacity());
            if let Some(p) = pivot_using {
                z.add(p.capacity());
            }
            z += vs(pivot_columns);
        }
        PropertySource::RelationshipCount { relationship } => z.add(relationship.capacity()),
        PropertySource::Pivot | PropertySource::DocblockTag => {}
    }
    z
}

fn laravel_meta(l: &LaravelMetadata) -> Sz {
    let mut z = Sz::default();
    z.add(size_of::<LaravelMetadata>());
    z += opt_ty(&l.factory_model);
    z += opt_ty(&l.custom_collection);
    z += opt_ty(&l.custom_builder);
    z.add(l.casts_definitions.capacity() * size_of::<(String, String)>());
    for (a, b) in &l.casts_definitions {
        z.add(a.capacity());
        z.add(b.capacity());
    }
    z += vs(&l.dates_definitions);
    z.add(l.attributes_definitions.capacity() * size_of::<(String, PhpType)>());
    for (a, t) in &l.attributes_definitions {
        z.add(a.capacity());
        z += ty(t);
    }
    z.add(l.attribute_defaults.capacity() * size_of::<(String, String)>());
    for (a, b) in &l.attribute_defaults {
        z.add(a.capacity());
        z.add(b.capacity());
    }
    z += vs(&l.column_names);
    for o in [
        &l.connection_name,
        &l.table_name,
        &l.primary_key,
        &l.key_type,
    ] {
        z += s(o);
    }
    if let Some(Some(x)) = &l.created_at_name {
        z.add(x.capacity());
    }
    if let Some(Some(x)) = &l.updated_at_name {
        z.add(x.capacity());
    }
    z.add(l.belongs_to_many_pivots.capacity() * size_of::<crate::types::PivotRelation>());
    for p in &l.belongs_to_many_pivots {
        z.add(p.method.capacity());
        if let Some(u) = &p.using {
            z.add(u.capacity());
        }
        z += vs(&p.columns);
    }
    z
}

#[derive(Default)]
struct Audit {
    seen: HashSet<usize>,

    n_classes: usize,
    cls_struct: Sz,
    cls_slots: Sz,
    cls_index: Sz,
    cls_names: Sz,
    cls_generics: Sz,
    cls_aliases: Sz,
    cls_traits: Sz,
    cls_docs: Sz,
    cls_laravel: Sz,

    n_methods: usize,
    m_struct: Sz,
    m_types: Sz,
    m_tmpl: Sz,
    m_docs: Sz,

    n_props: usize,
    p_struct: Sz,
    p_types: Sz,
    p_docs: Sz,
    p_src: Sz,

    n_consts: usize,
    c_struct: Sz,
    c_types: Sz,
    c_docs: Sz,
    c_vals: Sz,

    n_paramvecs: usize,
    n_params: usize,
    par_vec: Sz,
    par_types: Sz,
    par_docs: Sz,
    par_defaults: Sz,

    // ── Reclaim simulation counters ─────────────────────────────────
    /// `Option<PhpType>` / `PhpType` fields sitting inline in counted
    /// struct allocations (4 per method, 3 per param, …).
    pt_inline_slots: usize,
    /// Methods whose six display-only docblock fields are all empty.
    m_no_docs: usize,
    /// Parameter vectors with zero parameters (each still its own
    /// `Arc<Vec>` allocation today).
    empty_param_vecs: usize,
    /// Parameters with no description and no default-value text.
    par_no_docs: usize,
    /// Properties / constants with no display-only docblock data.
    p_no_docs: usize,
    c_no_docs: usize,
}

impl Audit {
    fn mark(&mut self, p: usize) -> bool {
        self.seen.insert(p)
    }

    fn total(&self) -> usize {
        [
            self.cls_struct,
            self.cls_slots,
            self.cls_index,
            self.cls_names,
            self.cls_generics,
            self.cls_aliases,
            self.cls_traits,
            self.cls_docs,
            self.cls_laravel,
            self.m_struct,
            self.m_types,
            self.m_tmpl,
            self.m_docs,
            self.p_struct,
            self.p_types,
            self.p_docs,
            self.p_src,
            self.c_struct,
            self.c_types,
            self.c_docs,
            self.c_vals,
            self.par_vec,
            self.par_types,
            self.par_docs,
            self.par_defaults,
        ]
        .iter()
        .map(|z| z.bytes)
        .sum()
    }

    fn param_slice(&mut self, params: &[ParameterInfo]) {
        for p in params {
            self.n_params += 1;
            // type_hint, native_type_hint, closure_this_type
            self.pt_inline_slots += 3;
            if p.description.is_none() && p.default_value.is_none() {
                self.par_no_docs += 1;
            }
            self.par_types += opt_ty(&p.type_hint);
            self.par_types += opt_ty(&p.native_type_hint);
            self.par_types += opt_ty(&p.closure_this_type);
            self.par_docs += s(&p.description);
            self.par_defaults += s(&p.default_value);
        }
    }

    fn params(&mut self, sv: &SharedVec<ParameterInfo>) {
        if !self.mark(sv.audit_ptr()) {
            return;
        }
        self.n_paramvecs += 1;
        if sv.is_empty() {
            self.empty_param_vecs += 1;
        }
        let mut z = Sz::default();
        z.add(ARC + VEC_HDR + sv.audit_capacity() * size_of::<ParameterInfo>());
        self.par_vec += z;
        self.param_slice(sv.as_slice());
    }

    fn method(&mut self, a: &Arc<MethodInfo>) {
        if !self.mark(Arc::as_ptr(a) as usize) {
            return;
        }
        self.n_methods += 1;
        let mut st = Sz::default();
        st.add(ARC + size_of::<MethodInfo>());
        self.m_struct += st;
        let m: &MethodInfo = a;
        // return_type, native_return_type, conditional_return, if_this_is, self_out
        self.pt_inline_slots += 5;
        if m.description.is_none()
            && m.return_description.is_none()
            && m.links.is_empty()
            && m.see_refs.is_empty()
            && m.deprecation_message.is_none()
            && m.deprecated_replacement.is_none()
        {
            self.m_no_docs += 1;
        }
        self.m_types += opt_ty(&m.return_type);
        self.m_types += opt_ty(&m.native_return_type);
        self.m_types += opt_ty(&m.conditional_return);
        self.m_types += opt_ty(&m.if_this_is);
        self.m_types += opt_ty(&m.self_out);
        self.m_types += ty_vec(&m.throws);
        self.m_types += assertions(&m.type_assertions);
        let mut tp = Sz::default();
        tp.add(m.template_params.capacity() * size_of::<Atom>());
        tp.add(m.template_bindings.capacity() * size_of::<(Atom, Atom)>());
        self.m_tmpl += tp;
        self.m_tmpl += atom_map_ty(&m.template_param_bounds);
        self.m_docs += s(&m.description);
        self.m_docs += s(&m.return_description);
        self.m_docs += vs(&m.links);
        self.m_docs += vs(&m.see_refs);
        self.m_docs += s(&m.deprecation_message);
        self.m_docs += s(&m.deprecated_replacement);
        self.params(&m.parameters);
    }

    fn property(&mut self, a: &Arc<PropertyInfo>) {
        if !self.mark(Arc::as_ptr(a) as usize) {
            return;
        }
        self.n_props += 1;
        let mut st = Sz::default();
        st.add(ARC + size_of::<PropertyInfo>());
        self.p_struct += st;
        let p: &PropertyInfo = a;
        // type_hint, native_type_hint
        self.pt_inline_slots += 2;
        if p.description.is_none()
            && p.see_refs.is_empty()
            && p.deprecation_message.is_none()
            && p.deprecated_replacement.is_none()
        {
            self.p_no_docs += 1;
        }
        self.p_types += opt_ty(&p.type_hint);
        self.p_types += opt_ty(&p.native_type_hint);
        self.p_docs += s(&p.description);
        self.p_docs += vs(&p.see_refs);
        self.p_docs += s(&p.deprecation_message);
        self.p_docs += s(&p.deprecated_replacement);
        if let Some(src) = &p.source {
            self.p_src += property_source(src);
        }
    }

    fn constant(&mut self, a: &Arc<ConstantInfo>) {
        if !self.mark(Arc::as_ptr(a) as usize) {
            return;
        }
        self.n_consts += 1;
        let mut st = Sz::default();
        st.add(ARC + size_of::<ConstantInfo>());
        self.c_struct += st;
        let c: &ConstantInfo = a;
        self.pt_inline_slots += 1;
        if c.description.is_none()
            && c.see_refs.is_empty()
            && c.deprecation_message.is_none()
            && c.deprecated_replacement.is_none()
        {
            self.c_no_docs += 1;
        }
        self.c_types += opt_ty(&c.type_hint);
        self.c_docs += s(&c.description);
        self.c_docs += vs(&c.see_refs);
        self.c_docs += s(&c.deprecation_message);
        self.c_docs += s(&c.deprecated_replacement);
        self.c_vals += s(&c.enum_value);
        self.c_vals += s(&c.value);
    }

    fn class(&mut self, a: &Arc<ClassInfo>) {
        if !self.mark(Arc::as_ptr(a) as usize) {
            return;
        }
        self.n_classes += 1;
        let mut st = Sz::default();
        st.add(ARC + size_of::<ClassInfo>());
        self.cls_struct += st;
        let c: &ClassInfo = a;

        if self.mark(c.methods.audit_ptr()) {
            let mut z = Sz::default();
            z.add(ARC + VEC_HDR + c.methods.audit_capacity() * size_of::<Arc<MethodInfo>>());
            self.cls_slots += z;
        }
        if self.mark(c.properties.audit_ptr()) {
            let mut z = Sz::default();
            z.add(ARC + VEC_HDR + c.properties.audit_capacity() * size_of::<Arc<PropertyInfo>>());
            self.cls_slots += z;
        }
        if self.mark(c.constants.audit_ptr()) {
            let mut z = Sz::default();
            z.add(ARC + VEC_HDR + c.constants.audit_capacity() * size_of::<Arc<ConstantInfo>>());
            self.cls_slots += z;
        }
        let mut idx = Sz::default();
        idx.add(VEC_HDR + c.method_index.capacity() * size_of::<(Atom, u32)>());
        self.cls_index += idx;

        let mut names = Sz::default();
        names.add(c.interfaces.capacity() * size_of::<Atom>());
        names.add(c.used_traits.capacity() * size_of::<Atom>());
        names.add(c.mixins.capacity() * size_of::<Atom>());
        names.add(c.require_implements.capacity() * size_of::<Atom>());
        self.cls_names += names;

        self.cls_generics += generic_pairs(&c.mixin_generics);
        self.cls_generics += generic_pairs(&c.extends_generics);
        self.cls_generics += generic_pairs(&c.implements_generics);
        self.cls_generics += generic_pairs(&c.use_generics);
        let mut tp = Sz::default();
        tp.add(c.template_params.capacity() * size_of::<Atom>());
        self.cls_generics += tp;
        self.cls_generics += atom_map_ty(&c.template_param_bounds);
        self.cls_generics += atom_map_ty(&c.template_param_defaults);

        self.cls_aliases += map_buckets::<Atom, TypeAliasDef>(c.type_aliases.capacity());
        for v in c.type_aliases.values() {
            match v {
                TypeAliasDef::Local(t) => self.cls_aliases += ty(t),
                TypeAliasDef::Import {
                    source_class,
                    original_name,
                } => {
                    let mut z = Sz::default();
                    z.add(source_class.capacity());
                    z.add(original_name.capacity());
                    self.cls_aliases += z;
                }
            }
        }

        let mut tr = Sz::default();
        tr.add(c.trait_precedences.capacity() * size_of::<TraitPrecedence>());
        for p in &c.trait_precedences {
            tr.add(p.insteadof.capacity() * size_of::<Atom>());
        }
        tr.add(c.trait_aliases.capacity() * size_of::<TraitAlias>());
        self.cls_traits += tr;

        self.cls_docs += s(&c.class_docblock);
        self.cls_docs += s(&c.deprecation_message);
        self.cls_docs += s(&c.deprecated_replacement);
        self.cls_docs += vs(&c.links);
        self.cls_docs += vs(&c.see_refs);

        // `@method` / `@property` tags parsed at extraction time.  The
        // methods are `Arc`-shared with every class that mixes them in, so
        // `self.method` de-duplicates them via the pointer set.
        if let Some(doc) = c.doc_members.as_ref()
            && self.mark(Arc::as_ptr(doc) as usize)
        {
            let mut z = Sz::default();
            z.add(ARC + size_of::<DocblockMembers>());
            z.add(VEC_HDR + doc.methods.capacity() * size_of::<Arc<MethodInfo>>());
            z.add(VEC_HDR + doc.properties.capacity() * size_of::<(Atom, Option<PhpType>)>());
            self.cls_docs += z;
            for (_, t) in &doc.properties {
                self.cls_docs += opt_ty(t);
            }
            for m in &doc.methods {
                self.method(m);
            }
        }

        if let Some(l) = &c.laravel {
            self.cls_laravel += laravel_meta(l);
        }

        for m in c.methods.iter() {
            self.method(m);
        }
        for p in c.properties.iter() {
            self.property(p);
        }
        for k in c.constants.iter() {
            self.constant(k);
        }
    }

    fn function(&mut self, f: &FunctionInfo) {
        // FunctionInfo values are stored inline in their maps (no Arc),
        // so struct bytes are attributed at the map-bucket level; only
        // walk the heap hanging off the fields.
        self.m_types += opt_ty(&f.return_type);
        self.m_types += opt_ty(&f.native_return_type);
        self.m_types += opt_ty(&f.conditional_return);
        self.m_types += ty_vec(&f.throws);
        self.m_types += assertions(&f.type_assertions);
        let mut tp = Sz::default();
        tp.add(f.template_params.capacity() * size_of::<Atom>());
        tp.add(f.template_bindings.capacity() * size_of::<(Atom, Atom)>());
        self.m_tmpl += tp;
        self.m_tmpl += atom_map_ty(&f.template_param_bounds);
        self.m_docs += s(&f.description);
        self.m_docs += s(&f.return_description);
        self.m_docs += vs(&f.links);
        self.m_docs += vs(&f.see_refs);
        self.m_docs += s(&f.deprecation_message);
        self.m_docs += s(&f.deprecated_replacement);
        self.m_docs += s(&f.namespace);
        self.params(&f.parameters);
        let mut ov = Sz::default();
        ov.add(f.overloads.capacity() * size_of::<Vec<ParameterInfo>>());
        for o in &f.overloads {
            ov.add(o.capacity() * size_of::<ParameterInfo>());
        }
        self.par_vec += ov;
        for o in &f.overloads {
            self.param_slice(o);
        }
    }

    fn print_members(&self, label: &str) {
        eprintln!("  [{label}]");
        eprintln!(
            "    classes: {} distinct | structs {:.1} MB | slot vecs {:.1} MB ({} allocs) | method_index {:.1} MB | name vecs {:.1} MB",
            self.n_classes,
            mb(self.cls_struct.bytes),
            mb(self.cls_slots.bytes),
            self.cls_slots.allocs,
            mb(self.cls_index.bytes),
            mb(self.cls_names.bytes),
        );
        eprintln!(
            "    class extras: generics {:.1} MB | aliases {:.1} MB | traits {:.1} MB | docs {:.1} MB | laravel {:.1} MB",
            mb(self.cls_generics.bytes),
            mb(self.cls_aliases.bytes),
            mb(self.cls_traits.bytes),
            mb(self.cls_docs.bytes),
            mb(self.cls_laravel.bytes),
        );
        eprintln!(
            "    methods: {} distinct | structs {:.1} MB | types {:.1} MB ({} allocs) | template {:.1} MB | docs {:.1} MB ({} allocs)",
            self.n_methods,
            mb(self.m_struct.bytes),
            mb(self.m_types.bytes),
            self.m_types.allocs,
            mb(self.m_tmpl.bytes),
            mb(self.m_docs.bytes),
            self.m_docs.allocs,
        );
        eprintln!(
            "    properties: {} distinct | structs {:.1} MB | types {:.1} MB | docs {:.1} MB | source {:.1} MB",
            self.n_props,
            mb(self.p_struct.bytes),
            mb(self.p_types.bytes),
            mb(self.p_docs.bytes),
            mb(self.p_src.bytes),
        );
        eprintln!(
            "    constants: {} distinct | structs {:.1} MB | types {:.1} MB | docs {:.1} MB | values {:.1} MB",
            self.n_consts,
            mb(self.c_struct.bytes),
            mb(self.c_types.bytes),
            mb(self.c_docs.bytes),
            mb(self.c_vals.bytes),
        );
        eprintln!(
            "    params: {} vecs, {} params | vec allocs {:.1} MB | types {:.1} MB ({} allocs) | docs {:.1} MB | defaults {:.1} MB",
            self.n_paramvecs,
            self.n_params,
            mb(self.par_vec.bytes),
            mb(self.par_types.bytes),
            self.par_types.allocs,
            mb(self.par_docs.bytes),
            mb(self.par_defaults.bytes),
        );
        eprintln!("    subtotal: {:.1} MB", mb(self.total()));
    }

    /// Turn the simulation counters into projected reclaim, so each
    /// candidate representation change can be ranked directly.
    fn print_reclaim(&self) {
        let heap_slots: usize = [
            self.m_types,
            self.p_types,
            self.c_types,
            self.par_types,
            self.m_tmpl,
            self.cls_generics,
            self.cls_aliases,
            self.cls_laravel,
        ]
        .iter()
        .map(|z| z.slots)
        .sum();
        let pt_total = heap_slots + self.pt_inline_slots;

        // Moving the six display-only docblock fields behind one
        // `Option<Box<…>>` frees their inline bytes on members that
        // carry no docs at all.
        let m_doc_fields = 4 * size_of::<Option<String>>() + 2 * size_of::<Vec<String>>();
        let p_doc_fields = 3 * size_of::<Option<String>>() + size_of::<Vec<String>>();
        let par_doc_fields = 2 * size_of::<Option<String>>();

        eprintln!("  [projected reclaim]");
        eprintln!(
            "    PhpType slots at {PT} B: {} ({} inline in structs + {} in heap allocs) → {:.1} MB",
            pt_total,
            self.pt_inline_slots,
            heap_slots,
            mb(pt_total * PT),
        );
        eprintln!(
            "    box docblock fields: {}/{} methods doc-free ({:.0}%) → {:.1} MB; {}/{} props → {:.1} MB; {}/{} params → {:.1} MB",
            self.m_no_docs,
            self.n_methods,
            100.0 * self.m_no_docs as f64 / self.n_methods.max(1) as f64,
            mb(self.m_no_docs * m_doc_fields),
            self.p_no_docs,
            self.n_props,
            mb(self.p_no_docs * p_doc_fields),
            self.par_no_docs,
            self.n_params,
            mb(self.par_no_docs * par_doc_fields),
        );
        eprintln!(
            "    share one empty SharedVec: {}/{} param vecs are empty → {:.1} MB",
            self.empty_param_vecs,
            self.n_paramvecs,
            mb(self.empty_param_vecs * (ARC + VEC_HDR)),
        );
    }
}

/// Bucket resident pages by mapping kind: file-backed (binary + libs),
/// glibc `[heap]`, `[stack]`, and anonymous (malloc arenas, mmap'd
/// allocations, thread stacks).
fn smaps_breakdown() -> (usize, usize, usize, usize) {
    let (mut file, mut heap, mut stack, mut anon) = (0usize, 0usize, 0usize, 0usize);
    let Ok(text) = std::fs::read_to_string("/proc/self/smaps") else {
        return (0, 0, 0, 0);
    };
    let mut current: u8 = 3; // 0=file 1=heap 2=stack 3=anon
    for line in text.lines() {
        let is_header = line
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_hexdigit())
            && line.contains('-')
            && line.split_whitespace().count() >= 5;
        if is_header {
            let path = line.split_whitespace().nth(5).unwrap_or("");
            current = if path == "[heap]" {
                1
            } else if path == "[stack]" {
                2
            } else if path.starts_with('/') {
                0
            } else {
                3
            };
        } else if let Some(rest) = line.strip_prefix("Rss:") {
            let kb: usize = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            match current {
                0 => file += kb * 1024,
                1 => heap += kb * 1024,
                2 => stack += kb * 1024,
                _ => anon += kb * 1024,
            }
        }
    }
    (file, heap, stack, anon)
}

fn read_proc_status() -> (usize, usize) {
    let mut rss = 0;
    let mut hwm = 0;
    if let Ok(text) = std::fs::read_to_string("/proc/self/status") {
        for line in text.lines() {
            let parse = |l: &str| {
                l.split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0)
                    * 1024
            };
            if line.starts_with("VmRSS:") {
                rss = parse(line);
            } else if line.starts_with("VmHWM:") {
                hwm = parse(line);
            }
        }
    }
    (rss, hwm)
}

/// Return free pages to the OS so the reported RSS approximates the
/// true live set rather than whatever the allocator is holding back.
///
/// Under mimalloc this is an explicit collect; under glibc's malloc it
/// is `malloc_trim`, which glibc otherwise only does opportunistically.
fn trim_heap() {
    #[cfg(all(feature = "mimalloc", target_os = "linux"))]
    unsafe {
        libmimalloc_sys::mi_collect(true);
    }
    #[cfg(all(
        target_env = "gnu",
        not(all(feature = "mimalloc", target_os = "linux"))
    ))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        unsafe {
            malloc_trim(0);
        }
    }
}

pub(crate) fn report(backend: &Backend, runner_content_bytes: usize) {
    eprintln!("\n══ PHPANTOM_MEM_AUDIT ══");
    eprintln!(
        "  build: target {} / {} libc, allocator {} — figures are only comparable across identical builds",
        std::env::consts::ARCH,
        if cfg!(target_env = "musl") {
            "musl"
        } else if cfg!(target_env = "gnu") {
            "gnu"
        } else {
            "native"
        },
        INNER_NAME,
    );
    eprintln!(
        "  sizeof: PhpType={PT} MethodInfo={} PropertyInfo={} ConstantInfo={} ParameterInfo={} FunctionInfo={} ClassInfo={} ShapeEntry={} TypeAssertion={}",
        size_of::<MethodInfo>(),
        size_of::<PropertyInfo>(),
        size_of::<ConstantInfo>(),
        size_of::<ParameterInfo>(),
        size_of::<FunctionInfo>(),
        size_of::<ClassInfo>(),
        size_of::<ShapeEntry>(),
        size_of::<TypeAssertion>(),
    );

    let mut a = Audit::default();

    // ── 1. Resolved-class cache ─────────────────────────────────────
    let mut cache_overhead = Sz::default();
    {
        let cache = backend.resolved_class_cache.read();
        let (map, fqn_keys, reverse_deps, substituted_bytes) = cache.audit_maps();
        cache_overhead += map_buckets::<(Atom, Vec<String>), Arc<ClassInfo>>(map.capacity());
        for (key, cls) in map {
            cache_overhead.add(key.1.capacity() * size_of::<String>());
            for g in &key.1 {
                cache_overhead.add(g.capacity());
            }
            a.class(cls);
        }
        cache_overhead += map_buckets::<String, HashSet<(Atom, Vec<String>)>>(fqn_keys.capacity());
        for (k, set) in fqn_keys {
            cache_overhead.add(k.capacity());
            cache_overhead += map_buckets::<(Atom, Vec<String>), ()>(set.capacity());
            for (_, gens) in set {
                cache_overhead.add(gens.capacity() * size_of::<String>());
                for g in gens {
                    cache_overhead.add(g.capacity());
                }
            }
        }
        cache_overhead +=
            map_buckets::<String, HashSet<(Atom, Vec<String>)>>(reverse_deps.capacity());
        for (k, set) in reverse_deps {
            cache_overhead.add(k.capacity());
            cache_overhead += map_buckets::<(Atom, Vec<String>), ()>(set.capacity());
            for (_, gens) in set {
                cache_overhead.add(gens.capacity() * size_of::<String>());
                for g in gens {
                    cache_overhead.add(g.capacity());
                }
            }
        }
        cache_overhead.add(substituted_bytes);
        eprintln!(
            "── resolved-class cache: {} entries, key+eviction+intern overhead {:.1} MB ({} allocs)",
            map.len(),
            mb(cache_overhead.bytes),
            cache_overhead.allocs,
        );
    }
    a.print_members("cache contents (deduped)");
    let cache_total = a.total() + cache_overhead.bytes;

    // ── 2. Unresolved class indexes (incremental over the cache) ────
    let before = a.total();
    let (n_cls, n_m) = (a.n_classes, a.n_methods);
    let mut idx_overhead = Sz::default();
    {
        let fqn_index = backend.symbols.fqn_class_index.read();
        idx_overhead.add(fqn_index.audit_heap(|_| 0));
        for (_, cls) in fqn_index.iter() {
            a.class(cls);
        }
    }
    {
        let uri_index = backend.symbols.uri_classes_index.read();
        idx_overhead += map_buckets::<String, Vec<Arc<ClassInfo>>>(uri_index.capacity());
        for (uri, classes) in uri_index.iter() {
            idx_overhead.add(uri.capacity());
            idx_overhead.add(classes.capacity() * size_of::<Arc<ClassInfo>>());
            for cls in classes {
                a.class(cls);
            }
        }
    }
    let mut store_keys = Sz::default();
    {
        let store = backend.symbols.method_store.read();
        store_keys += map_buckets::<(String, String), Arc<MethodInfo>>(store.capacity());
        for ((f, m_name), m) in store.iter() {
            store_keys.add(f.capacity());
            store_keys.add(m_name.capacity());
            a.method(m);
        }
    }
    eprintln!(
        "── unresolved indexes: +{} classes, +{} methods over cache | index overhead {:.1} MB | method_store keys+buckets {:.1} MB ({} allocs) | incremental content {:.1} MB",
        a.n_classes - n_cls,
        a.n_methods - n_m,
        mb(idx_overhead.bytes),
        mb(store_keys.bytes),
        store_keys.allocs,
        mb(a.total() - before),
    );

    // ── 3. Global functions and defines ────────────────────────────
    let before = a.total();
    let mut fn_overhead = Sz::default();
    let n_fns;
    {
        let funcs = backend.symbols.global_functions.read();
        n_fns = funcs.len();
        fn_overhead.add(funcs.audit_heap(|(uri, _)| uri.capacity()));
        for (_, (_, f)) in funcs.iter() {
            a.function(f);
        }
    }
    let mut def_overhead = Sz::default();
    {
        let defines = backend.symbols.global_defines.read();
        def_overhead += map_buckets::<String, crate::types::DefineInfo>(defines.capacity());
        for (k, d) in defines.iter() {
            def_overhead.add(k.capacity());
            def_overhead.add(d.file_uri.capacity());
            if let Some(v) = &d.value {
                def_overhead.add(v.capacity());
            }
        }
    }
    eprintln!(
        "── global functions: {} | map+uri overhead {:.1} MB | content {:.1} MB | defines overhead {:.1} MB",
        n_fns,
        mb(fn_overhead.bytes),
        mb(a.total() - before),
        mb(def_overhead.bytes),
    );

    // ── 4. Name/URI side indexes ────────────────────────────────────
    let mut side = Sz::default();
    {
        let m = backend.symbols.fqn_uri_index.read();
        side.add(m.audit_heap(|v| v.capacity()));
    }
    {
        let m = backend.symbols.fqn_origin_index.read();
        side.add(m.audit_heap(|_| 0));
    }
    eprintln!(
        "── fqn_uri_index + fqn_origin_index: {:.1} MB",
        mb(side.bytes)
    );

    let mut imports = Sz::default();
    {
        let m = backend.file_imports.read();
        imports += map_buckets::<String, HashMap<String, String>>(m.capacity());
        for (uri, im) in m.iter() {
            imports.add(uri.capacity());
            imports += map_buckets::<String, String>(im.capacity());
            for (k, v) in im {
                imports.add(k.capacity());
                imports.add(v.capacity());
            }
        }
    }
    let mut rnames = Sz::default();
    let mut n_rname_entries = 0usize;
    {
        let m = backend.resolved_names.read();
        rnames += map_buckets::<String, Arc<crate::names::OwnedResolvedNames>>(m.capacity());
        for (uri, rn) in m.iter() {
            rnames.add(uri.capacity());
            let (entries, bytes) = rn.audit_heap();
            n_rname_entries += entries;
            rnames.add(ARC + bytes);
        }
    }
    eprintln!(
        "── file_imports {:.1} MB ({} allocs) | resolved_names {:.1} MB ({} entries)",
        mb(imports.bytes),
        imports.allocs,
        mb(rnames.bytes),
        n_rname_entries,
    );

    // ── 5. Symbol maps ──────────────────────────────────────────────
    let mut sym = Sz::default();
    let mut span_structs = Sz::default();
    let mut span_strings = Sz::default();
    let mut member_idx = Sz::default();
    let mut n_spans = 0usize;
    let n_syms;
    {
        let m = backend.symbol_maps.read();
        n_syms = m.len();
        sym += map_buckets::<String, Arc<crate::symbol_map::SymbolMap>>(m.capacity());
        for (uri, sm) in m.iter() {
            sym.add(uri.capacity());
            sym.add(ARC + size_of::<crate::symbol_map::SymbolMap>());
            n_spans += sm.spans.len();
            span_structs.add(sm.spans.capacity() * size_of::<crate::symbol_map::SymbolSpan>());
            for sp in &sm.spans {
                span_strings.add(sp.kind.audit_heap());
            }
            sym.add(sm.spans.capacity() * size_of::<crate::symbol_map::SymbolSpan>());
            for sp in &sm.spans {
                sym.add(sp.kind.audit_heap());
            }
            sym += map_buckets::<Atom, Vec<usize>>(sm.member_access_indices.capacity());
            member_idx += map_buckets::<Atom, Vec<usize>>(sm.member_access_indices.capacity());
            for v in sm.member_access_indices.values() {
                sym.add(v.capacity() * size_of::<usize>());
                member_idx.add(v.capacity() * size_of::<usize>());
            }
            sym.add(sm.var_defs.capacity() * size_of::<crate::symbol_map::VarDefSite>());
            for v in &sm.var_defs {
                sym.add(v.name.capacity());
            }
            sym.add(sm.scopes.capacity() * 8);
            sym.add(sm.arrow_fn_scopes.capacity() * 4);
            sym.add(sm.body_scopes.capacity() * 8);
            sym.add(sm.narrowing_blocks.capacity() * 8);
            sym.add(sm.assert_narrowing_offsets.capacity() * 4);
            sym.add(sm.template_defs.capacity() * size_of::<crate::symbol_map::TemplateParamDef>());
            sym.add(sm.call_sites.capacity() * size_of::<crate::symbol_map::CallSite>());
            sym.add(sm.breakable_scopes.capacity() * 8);
            sym.add(sm.loop_scopes.capacity() * 8);
            sym.add(sm.switch_scopes.capacity() * 8);
            sym.add(sm.static_method_scopes.capacity() * 8);
            sym.add(sm.instance_method_scopes.capacity() * 8);
            sym.add(
                sm.untyped_closure_sites.capacity()
                    * size_of::<crate::symbol_map::UntypedClosureSite>(),
            );
            sym.add(
                sm.view_receiver_sites.capacity()
                    * size_of::<crate::symbol_map::ViewReceiverSite>(),
            );
            for site in &sm.view_receiver_sites {
                sym.add(site.key.capacity());
            }
        }
    }
    eprintln!(
        "── symbol_maps: {} files, {:.1} MB total | {} spans: structs {:.1} MB + strings {:.1} MB ({} allocs) | member_access_indices {:.1} MB | other vecs {:.1} MB",
        n_syms,
        mb(sym.bytes),
        n_spans,
        mb(span_structs.bytes),
        mb(span_strings.bytes),
        span_strings.allocs,
        mb(member_idx.bytes),
        mb(sym
            .bytes
            .saturating_sub(span_structs.bytes + span_strings.bytes + member_idx.bytes)),
    );

    // ── 6. Reference index ──────────────────────────────────────────
    // Per key, only the distinct contributing URIs are stored (as a
    // shared per-file `Arc<str>`) mapped to a non-declaration span
    // count, so there is no per-span duplication left to account for.
    let mut refs = Sz::default();
    let mut n_key_uri_pairs = 0usize;
    let n_resolved_member_files;
    let mut n_resolved_member_accesses = 0usize;
    let n_keys;
    let mut distinct_uris: HashSet<*const u8> = HashSet::new();
    {
        let idx = backend.reference_index.read();
        let (by_key, uri_keys, resolved_members) = idx.audit_maps();
        n_keys = by_key.len();
        refs += map_buckets::<crate::reference_index::ReferenceIndexKey, HashMap<Arc<str>, u32>>(
            by_key.capacity(),
        );
        for (k, inner) in by_key {
            refs.add(k.audit_heap());
            n_key_uri_pairs += inner.len();
            refs += map_buckets::<Arc<str>, u32>(inner.capacity());
            for uri in inner.keys() {
                distinct_uris.insert(Arc::as_ptr(uri).cast::<u8>());
            }
        }
        refs += map_buckets::<Arc<str>, Vec<crate::reference_index::ReferenceIndexKey>>(
            uri_keys.capacity(),
        );
        for (uri, keys) in uri_keys {
            refs.add(ARC + uri.len());
            refs.add(keys.capacity() * size_of::<crate::reference_index::ReferenceIndexKey>());
            for k in keys {
                refs.add(k.audit_heap());
            }
        }
        refs += map_buckets::<Arc<str>, Arc<crate::reference_index::ResolvedMemberFile>>(
            resolved_members.capacity(),
        );
        n_resolved_member_files = resolved_members.len();
        for (uri, file) in resolved_members {
            distinct_uris.insert(Arc::as_ptr(uri).cast::<u8>());
            let (accesses, bytes, allocations) = file.audit_heap();
            n_resolved_member_accesses += accesses;
            refs.add(ARC + size_of::<crate::reference_index::ResolvedMemberFile>());
            refs.add(bytes);
            refs.allocs += allocations;
        }
    }
    eprintln!(
        "── reference_index: {} keys, {} (key, uri) pairs, {} exact member accesses in {} files, {} distinct uris, {:.1} MB ({} allocs)",
        n_keys,
        n_key_uri_pairs,
        n_resolved_member_accesses,
        n_resolved_member_files,
        distinct_uris.len(),
        mb(refs.bytes),
        refs.allocs,
    );

    let (member_names, member_entries, member_locations, member_bytes, member_allocations) =
        backend.member_ref_counts.audit_heap();
    eprintln!(
        "── member_reference_cache: {} names, {} declarations, {} locations, {:.1} MB ({} allocs)",
        member_names,
        member_entries,
        member_locations,
        mb(member_bytes),
        member_allocations,
    );

    // ── 7. Remaining session stores ─────────────────────────────────
    let mut open = Sz::default();
    {
        let m = backend.open_files.read();
        for (uri, content) in m.iter() {
            open.add(uri.capacity());
            open.add(ARC + content.capacity());
        }
    }
    let mut misc = Sz::default();
    misc.add(backend.stub_index.read().audit_heap(|_| 0));
    misc.add(backend.stub_function_index.read().audit_heap(|_| 0));
    {
        let m = backend.stub_constant_index.read();
        misc += map_buckets::<&'static str, &'static str>(m.capacity());
    }
    {
        let m = backend.parsed_uris.read();
        misc += map_buckets::<String, ()>(m.capacity());
        for s in m.iter() {
            misc.add(s.capacity());
        }
    }
    misc.add(backend.symbols.class_not_found_cache.read().audit_heap());
    {
        let m = backend.symbols.uri_globals_index.read();
        misc += map_buckets::<String, crate::UriGlobals>(m.capacity());
        for (uri, (fns, consts)) in m.iter() {
            misc.add(uri.capacity());
            misc += vs(fns);
            misc += vs(consts);
        }
    }
    misc.add(
        backend
            .symbols
            .autoload_function_index
            .read()
            .audit_heap(|p| p.as_os_str().len()),
    );
    {
        let m = backend.symbols.autoload_constant_index.read();
        misc += map_buckets::<String, std::path::PathBuf>(m.capacity());
        for (k, p) in m.iter() {
            misc.add(k.capacity());
            misc.add(p.as_os_str().len());
        }
    }
    {
        let m = backend.symbols.autoload_file_paths.read();
        misc.add(m.capacity() * size_of::<std::path::PathBuf>());
        for p in m.iter() {
            misc.add(p.as_os_str().len());
        }
    }
    let mut laravel_keys = Sz::default();
    {
        let c = backend.laravel_string_key_cache.read();
        for v in [&c.config_keys, &c.view_names, &c.trans_keys]
            .into_iter()
            .flatten()
        {
            laravel_keys += vs(v);
        }
        if let Some(discovery) = &c.routes {
            if let Some(first) = discovery.routes.first() {
                laravel_keys.add(discovery.routes.capacity() * size_of_val(first));
            }
            for route in &discovery.routes {
                laravel_keys.add(route.name.capacity());
                laravel_keys.add(route.uri.capacity());
            }
            for prefix in &discovery.open_prefixes {
                laravel_keys.add(prefix.capacity());
            }
            for suffix in &discovery.open_suffixes {
                laravel_keys.add(suffix.capacity());
            }
        }
        if let Some(trees) = &c.config_trees {
            // ConfigNode is recursive; count the spine only.
            laravel_keys.add(trees.capacity() * 64);
            for (k, _) in trees {
                laravel_keys.add(k.capacity());
            }
        }
        if let Some(discovery) = &c.blade_discovery {
            for (name, path) in &discovery.views {
                laravel_keys.add(name.capacity() + path.as_os_str().len() + 48);
            }
            for map in [&discovery.components, &discovery.livewire] {
                for (name, fqn) in map {
                    laravel_keys.add(name.capacity() + fqn.capacity() + 48);
                }
            }
        }
        if let Some(groups) = &c.shared_view_vars {
            laravel_keys
                .add(groups.capacity() * size_of::<crate::blade::shared_vars::SharedVarGroup>());
            for group in groups.iter() {
                laravel_keys.add(group.views.capacity() * size_of::<String>());
                laravel_keys.add(group.vars.capacity() * size_of::<(String, String)>());
                for pattern in &group.views {
                    laravel_keys.add(pattern.capacity());
                }
                for (name, ty) in &group.vars {
                    laravel_keys.add(name.capacity() + ty.capacity());
                }
            }
        }
    }
    let mut blade = Sz::default();
    let n_blade;
    {
        let m = backend.blade_virtual_content.read();
        n_blade = m.len();
        misc += map_buckets::<String, String>(m.capacity());
        for (k, v) in m.iter() {
            blade.add(k.capacity());
            blade.add(v.capacity());
        }
    }
    {
        let m = backend.blade_injected_vars.read();
        misc += map_buckets::<String, crate::blade::call_site_inference::BladeScope>(m.capacity());
        for (k, scope) in m.iter() {
            blade.add(k.capacity());
            blade.add(scope.vars.capacity() * size_of::<(String, String)>());
            for (name, ty) in &scope.vars {
                blade.add(name.capacity());
                blade.add(ty.capacity());
            }
            if let Some(fqn) = &scope.this_class {
                blade.add(fqn.capacity());
            }
        }
    }
    eprintln!(
        "── stub/autoload/uri-globals/negative caches {:.1} MB | laravel string keys {:.1} MB | blade virtual content: {} files {:.1} MB",
        mb(misc.bytes),
        mb(laravel_keys.bytes),
        n_blade,
        mb(blade.bytes),
    );
    eprintln!(
        "── analyze runner file contents (transient, alive at audit): {:.1} MB",
        mb(runner_content_bytes),
    );

    let (rss_pre, _) = read_proc_status();
    trim_heap();
    let (rss, hwm) = read_proc_status();
    let (map_file, map_heap, map_stack, map_anon) = smaps_breakdown();
    eprintln!(
        "── resident by mapping (post-collect): file-backed {:.1} MB | [heap] {:.1} MB | [stack] {:.1} MB | anon (arenas + thread stacks) {:.1} MB",
        mb(map_file),
        mb(map_heap),
        mb(map_stack),
        mb(map_anon),
    );
    eprintln!(
        "── open_files {:.1} MB | ustr interner: {} entries, {:.1} MB | RSS before collect {:.1} MB",
        mb(open.bytes),
        ustr::num_entries(),
        mb(ustr::total_allocated()),
        mb(rss_pre),
    );

    let grand = a.total()
        + cache_overhead.bytes
        + idx_overhead.bytes
        + store_keys.bytes
        + fn_overhead.bytes
        + def_overhead.bytes
        + side.bytes
        + imports.bytes
        + rnames.bytes
        + sym.bytes
        + refs.bytes
        + open.bytes
        + misc.bytes
        + laravel_keys.bytes
        + blade.bytes
        + runner_content_bytes
        + ustr::total_allocated();
    eprintln!("── all members deduped across every store:");
    a.print_members("global totals");
    a.print_reclaim();

    TYPE_STATS.with(|s| {
        let s = s.borrow();
        let mut v: Vec<_> = s.variants.iter().map(|(k, n)| (*k, *n)).collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        let listed: Vec<String> = v
            .iter()
            .map(|(k, n)| format!("{k} {:.1}%", 100.0 * *n as f64 / s.total.max(1) as f64))
            .collect();
        eprintln!(
            "── PhpType occurrences walked: {} | distinct interned nodes: {} ({:.1}x sharing) | interner holds {}",
            s.total,
            s.distinct.len(),
            s.total as f64 / s.distinct.len().max(1) as f64,
            crate::php_type::interned_len(),
        );
        eprintln!("    variant mix: {}", listed.join(", "));
    });
    let (heap_bytes, heap_allocs) = live_heap();

    // ── 8. Drop-and-measure probe for stores without a walker ───────
    //
    // Runs last, after every walker, because it destroys the stores it
    // measures. Shared `Arc`s only show up in the store that releases
    // the final reference, so these are lower bounds for stores that
    // share data, and exact for stores that own theirs.
    eprintln!("── drop-and-measure (exact bytes released per store):");
    let probe = |label: &str, f: &mut dyn FnMut()| {
        let (before, before_n) = live_heap();
        f();
        let (after, after_n) = live_heap();
        eprintln!(
            "    {label}: {:.1} MB, {} allocations",
            mb(before.saturating_sub(after)),
            before_n.saturating_sub(after_n),
        );
    };
    probe("blade_source_maps", &mut || {
        backend.blade_source_maps.write().clear()
    });
    probe("blade_virtual_content", &mut || {
        backend.blade_virtual_content.write().clear()
    });
    probe("schema_index", &mut || {
        *backend.schema_index.write() = Default::default()
    });
    probe("laravel_macros", &mut || {
        *backend.laravel_macros.write() = Default::default()
    });
    probe("laravel_provider_resources", &mut || {
        *backend.laravel_provider_resources.write() = Default::default()
    });
    probe("laravel_provider_scans", &mut || {
        *backend.laravel_provider_scans.write() = Default::default()
    });
    probe("laravel_string_key_cache", &mut || {
        *backend.laravel_string_key_cache.write() = Default::default()
    });
    probe("member_completion_cache", &mut || {
        backend.member_completion_cache.lock().clear()
    });
    probe("member_reference_cache", &mut || {
        backend.member_ref_counts.clear_cached()
    });
    probe("auth_user_type_cache", &mut || {
        backend.auth_user_type_cache.write().clear()
    });
    probe("storage_disk_type_cache", &mut || {
        *backend.storage_disk_type_cache.write() = None
    });
    probe("parse_errors", &mut || backend.parse_errors.write().clear());
    probe("file_namespaces", &mut || {
        backend.file_namespaces.write().clear()
    });
    probe("resolved_names", &mut || {
        backend.resolved_names.write().clear()
    });
    probe("file_imports", &mut || backend.file_imports.write().clear());
    probe("symbol_maps", &mut || backend.symbol_maps.write().clear());
    probe("reference_index", &mut || {
        let uris: Vec<Arc<str>> = backend
            .reference_index
            .read()
            .audit_maps()
            .1
            .keys()
            .cloned()
            .collect();
        for uri in uris {
            backend.evict_reference_index_uri(&uri);
        }
    });
    probe("method_store", &mut || {
        backend.symbols.method_store.write().clear()
    });
    probe("resolved_class_cache", &mut || {
        backend.resolved_class_cache.write().clear()
    });
    probe("fqn_class_index", &mut || {
        backend.symbols.fqn_class_index.write().clear()
    });
    probe("uri_classes_index", &mut || {
        backend.symbols.uri_classes_index.write().clear()
    });
    probe("global_functions", &mut || {
        backend.symbols.global_functions.write().clear()
    });
    probe("global_defines", &mut || {
        backend.symbols.global_defines.write().clear()
    });
    probe("gti_index", &mut || {
        backend.symbols.gti_index.write().clear()
    });
    probe("uri_globals_index", &mut || {
        backend.symbols.uri_globals_index.write().clear()
    });
    probe("fqn_uri + fqn_origin index", &mut || {
        backend.symbols.fqn_uri_index.write().clear();
        backend.symbols.fqn_origin_index.write().clear();
    });
    probe("autoload indexes", &mut || {
        backend.symbols.autoload_function_index.write().clear();
        backend
            .symbols
            .autoload_function_origin_index
            .write()
            .clear();
        backend.symbols.autoload_constant_index.write().clear();
        backend
            .symbols
            .autoload_constant_origin_index
            .write()
            .clear();
        backend.symbols.autoload_file_paths.write().clear();
    });
    probe("class_not_found_cache", &mut || {
        backend.symbols.class_not_found_cache.write().clear()
    });
    probe("stub indexes", &mut || {
        backend.stub_index.write().clear();
        backend.stub_function_index.write().clear();
        backend.stub_constant_index.write().clear();
    });
    probe("parsed_uris", &mut || backend.parsed_uris.write().clear());
    probe("laravel_aliases", &mut || {
        *backend.laravel_aliases.write() = None
    });
    probe("laravel seed/mixin/pivot/command sets", &mut || {
        backend.laravel_macro_seeds.write().clear();
        backend.laravel_macro_mixin_uris.write().clear();
        backend.laravel_date_seed_uris.write().clear();
        *backend.laravel_pivots.write() = Default::default();
        *backend.laravel_commands.write() = Default::default();
        *backend.laravel_gates.write() = Default::default();
        backend.laravel_runtime_config_keys.write().clear();
    });
    probe("blade_uris", &mut || backend.blade_uris.write().clear());
    probe("phar_archives", &mut || {
        backend.phar_archives.write().clear()
    });
    probe("open_files", &mut || backend.open_files.write().clear());
    probe("did_change_parse_locks", &mut || {
        backend.did_change_parse_locks.lock().clear()
    });
    probe("workspace psr4 + vendor paths", &mut || {
        backend.workspace.psr4_mappings.write().clear();
        backend.workspace.vendor_uri_prefixes.lock().clear();
        backend.workspace.vendor_dir_paths.lock().clear();
        backend
            .workspace
            .vendor_package_origin_roots
            .write()
            .clear();
    });
    probe("diagnostic state", &mut || {
        backend.diag.pending_uris.lock().clear();
        backend.diag.last_slow.lock().clear();
        backend.diag.last_fast.lock().clear();
        backend.diag.last_full.lock().clear();
        backend.diag.result_ids.lock().clear();
        backend.diag.suppressed.lock().clear();
        backend.diag.assemble_locks.lock().clear();
        *backend.diag.workspace_diags.lock() = Default::default();
    });

    let (residual_bytes, residual_allocs) = live_heap();
    eprintln!(
        "    ── residual after every probe: {:.1} MB in {} allocations (runner file contents {:.1} MB; the rest is ustr, thread-local caches, and runtime state)",
        mb(residual_bytes),
        residual_allocs,
        mb(runner_content_bytes),
    );

    eprintln!(
        "── audited live total {:.1} MB (cache section {:.1} MB) | VmRSS after collect {:.1} MB | VmHWM {:.1} MB",
        mb(grand),
        mb(cache_total),
        mb(rss),
        mb(hwm),
    );
    eprintln!(
        "── allocator: {:.1} MB live in {} live allocations (avg {:.0} B) | audit covers {:.0}% of live heap | allocator+page overhead {:.1} MB ({:.0}% of RSS)",
        mb(heap_bytes),
        heap_allocs,
        heap_bytes as f64 / heap_allocs.max(1) as f64,
        100.0 * grand as f64 / heap_bytes.max(1) as f64,
        mb(rss.saturating_sub(heap_bytes)),
        100.0 * rss.saturating_sub(heap_bytes) as f64 / rss.max(1) as f64,
    );
    eprintln!("══ end PHPANTOM_MEM_AUDIT ══\n");
}
