/// Type resolution for completion subjects.
///
/// This module contains the core entry points for resolving a completion
/// subject (e.g. `$this`, `self`, `static`, `$var`, `$this->prop`,
/// `ClassName`) to a concrete `ClassInfo` so that the correct completion
/// items can be offered.
///
/// The resolution logic is split across several sibling modules:
///
/// - [`super::call_resolution`]: Call expression and callable target
///   resolution (method calls, static calls, function calls, constructor
///   calls, signature help, named-argument completion).
/// - [`super::type_resolution`]: Type-hint string to `ClassInfo` mapping
///   (unions, intersections, generics, type aliases, object shapes).
/// - [`crate::completion::source::helpers`]: Source-text scanning helpers
///   (closure return types, first-class callable resolution, `new`
///   expression parsing, array access segment walking).
/// - [`super::variable::resolution`]: Variable type resolution via
///   assignment scanning and parameter type hints.
/// - [`super::types::narrowing`]: instanceof / assert / custom type guard
///   narrowing.
/// - [`super::variable::closure_resolution`]: Closure and arrow-function
///   parameter resolution.
/// - [`crate::inheritance`]: Class inheritance merging (traits, mixins,
///   parent chain).
/// - [`super::conditional_resolution`]: PHPStan conditional return type
///   resolution at call sites.
///
/// Context types ([`ResolutionCtx`], [`VarResolutionCtx`], [`Loaders`]) and
/// the thread-local chain resolution cache live in [`context`].
mod context;

pub(crate) use context::{
    CtxLoaders, FunctionLoaderFn, LendsLoaders, Loaders, OwnedLoaders, ResolutionCtx,
    ScopeVarResolverFn, VarResolutionCtx, with_chain_resolution_cache, with_isolated_chain_cache,
};

use crate::atom::atom;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::{find_class_by_name, is_self_or_static, resolve_class_keyword};
use crate::docblock;
use crate::inheritance::resolve_property_type_hint;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::subject_expr::BracketSegment;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::*;
use crate::virtual_members::resolve_class_fully_maybe_cached;

use context::{CHAIN_CACHE, ChainKey, ChainSite, resolved_to_arcs};

/// Resolve a completion subject to all candidate types, preserving
/// both class info and type strings.
///
/// This is the primary entry point for subject resolution.  It returns
/// `Vec<ResolvedType>` which carries both the structured type string
/// (e.g. `PhpType::named("Collection")`) and the optional `ClassInfo`.
/// Callers that only need classes can call
/// `ResolvedType::into_arced_classes()` on the result.
pub(crate) fn resolve_target_classes(
    subject: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let expr = SubjectExpr::parse(subject);
    resolve_target_classes_expr(&expr, access_kind, ctx)
}

/// Core dispatch for [`resolve_target_classes`], operating on a
/// pre-parsed [`SubjectExpr`].
///
/// A method or property chain nests one node per link, and each link's
/// child is the link before it — a chain, not a tree.  Resolving the
/// outermost link and recursing into its base would therefore cost a stack
/// frame per link, and a generated fluent chain has no length bound, so the
/// spine is collected into a `Vec` and resolved outward from its base
/// instead.
pub(crate) fn resolve_target_classes_expr(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // `spine[0]` is the expression itself, `spine[len - 1]` its base.  Each
    // entry carries the access kind its parent link resolves it with: a
    // method call always reads its receiver through `->`, while a property
    // chain passes its own access kind down.
    let mut spine: Vec<(&SubjectExpr, AccessKind)> = vec![(expr, access_kind)];
    loop {
        let &(node, node_access) = spine.last().expect("spine is seeded with `expr`");
        let next = match node {
            SubjectExpr::CallExpr { callee, .. } => match callee.as_ref() {
                SubjectExpr::MethodCall { base, .. } => Some((base.as_ref(), AccessKind::Arrow)),
                _ => None,
            },
            SubjectExpr::PropertyChain { base, .. } => Some((base.as_ref(), node_access)),
            _ => None,
        };
        match next {
            Some(entry) => spine.push(entry),
            None => break,
        }
    }

    let keys = SpineKeys::new(&spine, ctx);

    // Probe the outermost link inward for an answer that does not depend on
    // the receiver: a cached result, or a type the forward walker narrowed
    // the link's path to.  The first hit is a finished result for that
    // link, so nothing below it needs resolving at all.
    let mut receiver: Option<Vec<ResolvedType>> = None;
    let mut unresolved = spine.len();
    for index in 0..spine.len() {
        let hit = keys
            .cache_key(index)
            .and_then(lookup_chain_cache)
            .or_else(|| {
                keys.scope_key(index)
                    .and_then(|key| lookup_scope_for_subject(key, ctx))
            });
        let Some(hit) = hit else { continue };
        if index == 0 {
            return hit;
        }
        receiver = Some(hit);
        unresolved = index;
        break;
    }

    for index in (0..unresolved).rev() {
        let (node, node_access) = spine[index];
        receiver = Some(resolve_chain_link(
            node,
            node_access,
            keys.cache_key(index),
            receiver.take(),
            ctx,
        ));
    }
    receiver.unwrap_or_default()
}

/// Resolve one link of a receiver spine, reading from and writing to the
/// chain cache under `cache_key` when the link is cacheable.
fn resolve_chain_link(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    cache_key: Option<&ChainKey>,
    receiver: Option<Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let Some(cache_key) = cache_key else {
        return resolve_target_classes_expr_inner(expr, access_kind, receiver, ctx);
    };
    if let Some(hit) = lookup_chain_cache(cache_key) {
        return hit;
    }
    let result = resolve_target_classes_expr_inner(expr, access_kind, receiver, ctx);
    CHAIN_CACHE.with(|cell| {
        if let Some(ref mut cache) = *cell.borrow_mut() {
            cache.entries.insert(cache_key.clone(), result.clone());
        }
    });
    result
}

/// Whether `var_name` is a plain `$variable` name rather than a complex
/// expression (a property/static chain, array access, a comparison or
/// boolean operator) that a scope entry could never match.
///
/// The scope-reading fast path is built for bare variables; anything
/// else falls through to it wastefully, so callers guard on this before
/// taking that path.
fn is_bare_variable(var_name: &str) -> bool {
    !var_name.contains("->")
        && !var_name.contains("::")
        && !var_name.contains('[')
        && !var_name.contains("===")
        && !var_name.contains("&&")
        && !var_name.contains("??")
        && !var_name.contains("||")
}

fn lookup_chain_cache(cache_key: &ChainKey) -> Option<Vec<ResolvedType>> {
    CHAIN_CACHE.with(|cell| {
        let borrow = cell.borrow();
        borrow
            .as_ref()
            .and_then(|cache| cache.entries.get(cache_key).cloned())
    })
}

/// The keys each link of a receiver spine is looked up under: its chain
/// cache key and, when the forward walker can narrow it, its scope key.
///
/// Every link's subject text is a prefix of the outermost link's, and so,
/// mostly, is its scope key, so both are rendered once for the whole spine
/// and sliced per link.  What a link's key depends on below it is likewise
/// carried up from the base one link at a time.  Working any of it out per
/// link instead would cost the length of the chain at every link, and a
/// generated fluent chain runs to hundreds of links.
struct SpineKeys {
    /// The outermost link's subject text; link `i`'s is `text[..ends[i]]`.
    text: String,
    ends: Vec<usize>,
    /// The base's scope key followed by the text the links add to it, when
    /// the base is spelled differently as a key than as text (`$rows["0"]`
    /// against `$rows[0]`).  The scope key of a link whose key runs down
    /// to the base is a prefix of this rather than of `text`.
    rebased: Option<String>,
    /// Per link: whether its scope key runs through every link below it
    /// down to the base, rather than stopping at a call with arguments.
    reaches_base: Vec<bool>,
    /// Per link: whether the forward walker can hold a narrowed type for it.
    narrowable: Vec<bool>,
    cache_keys: Vec<Option<ChainKey>>,
}

impl SpineKeys {
    fn new(spine: &[(&SubjectExpr, AccessKind)], ctx: &ResolutionCtx<'_>) -> Self {
        let base_index = spine.len() - 1;
        let base = spine[base_index].0;

        // What the links' keys depend on below them, carried up from the
        // base.  A link's scope key descends into the link below it when
        // it is a property or an argument-less call (see
        // `SubjectExpr::scope_key_base`); a call with arguments is keyed
        // under its own text.
        let mut reaches_base = vec![true; spine.len()];
        let mut narrowable = vec![false; spine.len()];
        // Whether a link roots in a variable through the links its scope
        // key descends (`scope_key_roots_in_variable`), and through
        // property and array-access links alone (`base_roots_in_variable`).
        let mut key_rooted = base.scope_key_roots_in_variable();
        let mut path_rooted = vec![base_roots_in_variable(base); spine.len()];
        narrowable[base_index] = is_narrowable_call(base);
        for index in (0..base_index).rev() {
            let (descends, is_property) = match spine[index].0 {
                SubjectExpr::CallExpr { args_text, .. } => (args_text.trim().is_empty(), false),
                _ => (true, true),
            };
            reaches_base[index] = descends && reaches_base[index + 1];
            narrowable[index] = is_property || (descends && key_rooted);
            key_rooted = descends && key_rooted;
            path_rooted[index] = is_property && path_rooted[index + 1];
        }

        // Which links the chain cache takes, and on what terms (see
        // `cache_keys`), while the cache is active.
        let caching = CHAIN_CACHE.with(|cell| cell.borrow().is_some());
        let positional_allowed = ctx.scope_var_resolver.is_none()
            && !crate::type_engine::variable::forward_walk::is_building_scopes();
        let cacheable: Vec<Option<bool>> = (0..spine.len())
            .map(|index| {
                if !caching {
                    return None;
                }
                let node = spine[index].0;
                let shared = match node {
                    SubjectExpr::CallExpr { .. } => !narrowable[index],
                    SubjectExpr::MethodCall { .. }
                    | SubjectExpr::StaticMethodCall { .. }
                    | SubjectExpr::StaticAccess { .. } => true,
                    // PropertyChain is only shared when its base does NOT
                    // root in a variable — e.g. `$this->method()->prop`
                    // (rooted in a call).
                    SubjectExpr::PropertyChain { .. } => !path_rooted[index + 1],
                    _ => false,
                };
                let positional = !shared
                    && positional_allowed
                    && matches!(
                        node,
                        SubjectExpr::CallExpr { .. } | SubjectExpr::PropertyChain { .. }
                    );
                (shared || positional).then_some(shared)
            })
            .collect();

        let mut keys = SpineKeys {
            text: String::new(),
            ends: vec![0; spine.len()],
            rebased: None,
            reaches_base,
            narrowable,
            cache_keys: vec![None; spine.len()],
        };
        if !keys.narrowable.iter().any(|&n| n) && cacheable.iter().all(Option::is_none) {
            return keys;
        }

        let text = &mut keys.text;
        text.push_str(&base.to_subject_text());
        keys.ends[base_index] = text.len();
        for index in (0..base_index).rev() {
            match spine[index].0 {
                SubjectExpr::CallExpr { callee, args_text } => {
                    let SubjectExpr::MethodCall { method, .. } = callee.as_ref() else {
                        unreachable!("the spine only descends into method calls")
                    };
                    text.push_str("->");
                    text.push_str(method);
                    text.push('(');
                    // Blank arguments are spelled `()`, as the scope key
                    // spells them.
                    if !args_text.trim().is_empty() {
                        text.push_str(args_text);
                    }
                    text.push(')');
                }
                SubjectExpr::PropertyChain { property, .. } => {
                    text.push_str("->");
                    text.push_str(property);
                }
                _ => unreachable!("the spine only descends through calls and properties"),
            }
            keys.ends[index] = text.len();
        }

        if keys.narrowable.iter().any(|&n| n) {
            let base_key = subject_scope_key(base);
            let base_text = &keys.text[..keys.ends[base_index]];
            if base_key != base_text {
                let mut rebased = base_key;
                rebased.push_str(&keys.text[keys.ends[base_index]..]);
                keys.rebased = Some(rebased);
            }
        }

        if caching {
            keys.cache_keys = Self::cache_keys(spine, &keys.text, &keys.ends, &cacheable, ctx);
        }
        keys
    }

    /// The chain cache key of every link, `None` for a link that must not
    /// be cached.  `cacheable` says which links the cache takes, and
    /// whether each is shared across positions (`Some(true)`) or keyed to
    /// the one it is written at (`Some(false)`).
    ///
    /// The cache is NOT used for variable-only subjects (no `->` or `::`
    /// in the expression): those are answered straight from the scope.
    ///
    /// A chain rooted in a variable (`$this->pet`, `$obj->prop`,
    /// `$args[0]->value`, `$this->a->b`), and an argument-less instance
    /// call on one (`$job->data()`), resolves differently wherever the
    /// variable or a path through it was narrowed or written: `$this->pet`
    /// may be a `Dog` inside `if ($this->pet instanceof Dog)` and a `Cat`
    /// after `if (!$this->pet instanceof Cat) { return; }`.  So its text
    /// alone does not identify the answer, but its text where it is
    /// written does, whenever the scope at that position is fixed: outside
    /// a walk in progress, which can see one position on several scopes
    /// (each re-walk of a loop body).
    ///
    /// During a diagnostic pass the scope at a position is the snapshot the
    /// position reads, so the occurrences reading the same snapshot share
    /// an entry: every member access in one statement asks about the
    /// prefixes of the same chain, and would otherwise resolve each of them
    /// again.
    ///
    /// Static accesses and calls that carry arguments are safe to share
    /// across positions: their return types are deterministic (method
    /// signatures don't change based on narrowing context).
    fn cache_keys(
        spine: &[(&SubjectExpr, AccessKind)],
        text: &str,
        ends: &[usize],
        cacheable: &[Option<bool>],
        ctx: &ResolutionCtx<'_>,
    ) -> Vec<Option<ChainKey>> {
        let base_index = spine.len() - 1;
        let mut keys = vec![None; spine.len()];

        // Each link is numbered from the link below it, so the whole spine
        // is numbered even where some links go uncached.
        let mut chains = vec![0u32; spine.len()];
        CHAIN_CACHE.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let Some(cache) = borrow.as_mut() else {
                return;
            };
            let mut scratch = String::new();
            let mut parent = None;
            let mut start = 0;
            for index in (0..spine.len()).rev() {
                let id = cache.intern(parent, &text[start..ends[index]], &mut scratch);
                chains[index] = id;
                parent = Some(id);
                start = ends[index];
            }
        });

        // The same subject text can mean different classes in different
        // files (`use A\Pen;` in one, `use B\Pen;` in another, both
        // spelling `Pen::make()`), and some activations of the chain cache
        // — the reference-count pending-item loop, find-references/rename
        // — span every file the request walks, not just one.
        // `ctx.content` is borrowed once per file for as long as any chain
        // from that file is being resolved, so its pointer is a cheap
        // per-file discriminator: the same technique
        // `resolve_variable_types`'s re-entry guards use for the same
        // reason (see `variable/resolution.rs`).  Two ResolutionCtx built
        // from the same file share `content` and thus the pointer, so
        // within-file sharing (the cache's actual purpose) is unaffected.
        let file_id = ctx.content.as_ptr() as usize;

        // A chain that references a local variable (as receiver or as a
        // call argument) can resolve to different types at call sites
        // where the variable holds a different type — e.g.
        // `$this->parse($stmt)` where `$stmt` is a different subtype in two
        // methods, or a `@template T` method binding `@return T` from a
        // variable argument.  Keying by the subject text alone would leak
        // the result across sites, so mix in a discriminator built from
        // those variables' resolved types: sites where the variables share
        // a type still share the cache entry (so the common case stays
        // fast), while differently-typed sites get distinct entries.  When
        // the variables can't be resolved cheaply (no active scope), fall
        // back to a per-site key so nothing leaks.  Chains with no local
        // variables keep the shared text-only key.
        //
        // A link's variables are those of the link below it plus the ones
        // its own arguments name, so they are gathered on the way up, and
        // the discriminator is only rebuilt when a link adds one.
        let mut vars = Vec::new();
        spine[base_index].0.collect_local_variables(&mut vars);
        let mut discriminator: Option<(usize, Option<std::rc::Rc<str>>)> = None;

        for index in (0..spine.len()).rev() {
            if index < base_index
                && let SubjectExpr::CallExpr { args_text, .. } = spine[index].0
            {
                crate::type_engine::subject_expr::collect_text_local_variables(
                    args_text, &mut vars,
                );
            }
            let Some(shared) = cacheable[index] else {
                continue;
            };
            let site = if !shared {
                match crate::type_engine::variable::forward_walk::diagnostic_scope_floor(
                    ctx.cursor_offset,
                ) {
                    Some(floor) => ChainSite::Scope(floor),
                    None => ChainSite::At(ctx.cursor_offset),
                }
            } else if vars.is_empty() {
                ChainSite::Anywhere
            } else {
                let disc = match &discriminator {
                    Some((len, disc)) if *len == vars.len() => disc.clone(),
                    _ => {
                        let disc = scope_type_discriminator(&vars, ctx).map(std::rc::Rc::from);
                        discriminator = Some((vars.len(), disc.clone()));
                        disc
                    }
                };
                match disc {
                    Some(disc) => ChainSite::Typed(disc),
                    None => ChainSite::At(ctx.cursor_offset),
                }
            };
            keys[index] = Some(ChainKey {
                file_id,
                chain: chains[index],
                site,
            });
        }
        keys
    }

    fn cache_key(&self, index: usize) -> Option<&ChainKey> {
        self.cache_keys[index].as_ref()
    }

    /// The forward-walker scope key of link `index`, or `None` when the
    /// walker never narrows that link.
    fn scope_key(&self, index: usize) -> Option<&str> {
        if !self.narrowable[index] {
            return None;
        }
        let base_end = self.ends[self.ends.len() - 1];
        match &self.rebased {
            Some(rebased) if self.reaches_base[index] => {
                let base_key_len = rebased.len() - (self.text.len() - base_end);
                Some(&rebased[..base_key_len + self.ends[index] - base_end])
            }
            _ => Some(&self.text[..self.ends[index]]),
        }
    }
}

/// Inner implementation of [`resolve_target_classes_expr`] without
/// chain caching.  The outer function handles cache lookup/store and the
/// spine walk, so `receiver` arrives already resolved for every link but
/// the base.
///
/// Recursion here follows the finite structure of the subject
/// expression plus variable resolution, which carries its own keyed
/// re-entry guards; class resolution cannot re-enter it (cycles are
/// broken inside `resolve_class_fully`), so no depth cap is needed.
fn resolve_target_classes_expr_inner(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    receiver: Option<Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.all_classes;
    let class_loader = ctx.class_loader;

    match expr {
        // ── Keywords that always mean "current class" ────────────
        SubjectExpr::This => {
            // `$this` is not available inside static methods.
            if current_class.is_some() && ctx.is_in_static_method {
                return vec![];
            }

            // Consult the forward-walk scope for a narrowed or seeded
            // `$this` type.  This covers two cases the lexical
            // `current_class` fallback cannot:
            //   - `assert($this instanceof X)` inside a top-level closure
            //     (e.g. a Pest test) where there is no enclosing class,
            //     so `current_class` is `None`.
            //   - `instanceof` narrowing of `$this` to a subclass inside
            //     a regular method body.
            // When the scope yields nothing, fall back to the lexical
            // `current_class` below.
            let from_scope = resolve_this_from_scope(ctx);

            // `@param-closure-this` override: when the cursor is inside a
            // closure passed as an argument to a function whose parameter
            // carries the tag, `$this` is the declared type rather than the
            // lexical class.  The tag states what the closure is *bound* to,
            // so a narrowing proof inside the body still refines it — a
            // `assert($this instanceof AppTestCase)` in a Pest closure whose
            // `test()` parameter declares `@param-closure-this TestCase`
            // means `$this` is the subclass.  The scope only wins when it is
            // strictly narrower; otherwise it holds the lexically captured
            // `$this` the tag is there to replace.
            if let Some(bound) = super::variable::closure_resolution::find_closure_this_types(ctx) {
                let narrowed = from_scope.filter(|types| {
                    !types.is_empty()
                        && types.iter().all(|rt| {
                            rt.class_info.as_ref().is_some_and(|ci| {
                                bound.iter().any(|b| {
                                    b.class_info.as_ref().is_some_and(|bound_cls| {
                                        crate::class_lookup::is_subclass_of(
                                            &ci.fqn(),
                                            &bound_cls.fqn(),
                                            class_loader,
                                        )
                                    })
                                })
                            })
                        })
                });
                return narrowed.unwrap_or(bound);
            }

            let mut this_types = if let Some(scope_types) = from_scope {
                scope_types
            } else {
                current_class
                    .map(|cc| ResolvedType::from_class(cc.clone()))
                    .into_iter()
                    .collect()
            };

            // Inside a trait, `$this` is the class using the trait, never
            // the trait itself — see `trait_context` for what stands in for
            // it when the using class is not the one being analysed.
            if let Some(cc) = current_class
                && cc.kind == ClassLikeKind::Trait
            {
                crate::type_engine::trait_context::extend_this_with_trait_bounds(
                    &mut this_types,
                    cc,
                    all_classes,
                    class_loader,
                    ctx.backend,
                );
            }

            this_types
        }
        SubjectExpr::SelfKw | SubjectExpr::StaticKw => resolve_self_static_class(ctx)
            .map(ResolvedType::from_class)
            .into_iter()
            .collect(),

        // ── `parent::` — resolve to the current class's parent ──
        SubjectExpr::Parent => {
            if let Some(cc) = current_class
                && let Some(ref parent_name) = cc.parent_class
            {
                if let Some(cls) = find_class_by_name(all_classes, parent_name) {
                    return vec![ResolvedType::from_arc(Arc::clone(cls))];
                }
                return class_loader(parent_name)
                    .map(ResolvedType::from_arc)
                    .into_iter()
                    .collect();
            }
            vec![]
        }

        // ── Inline array literal with index access ──────────────
        SubjectExpr::InlineArray { elements, .. } => {
            let mut element_types = Vec::new();
            for elem_text in elements {
                let elem = elem_text.trim();
                if elem.is_empty() {
                    continue;
                }
                let elem_expr = SubjectExpr::parse(elem);
                let resolved = resolve_target_classes_expr(&elem_expr, AccessKind::Arrow, ctx);
                ResolvedType::extend_unique(&mut element_types, resolved);
            }
            element_types
        }

        // ── Enum case / static member access ────────────────────
        SubjectExpr::StaticAccess { class, member } => {
            // Handle self/static/parent keywords — SubjectExpr::parse
            // produces StaticAccess for "self::MONTH", "static::FOO",
            // etc., but "self"/"static"/"parent" are keywords, not
            // class names, so find_class_by_name / class_loader won't
            // find them.
            let owner_classes: Vec<Arc<ClassInfo>> = if is_self_or_static(class) {
                resolve_self_static_class(ctx)
                    .map(Arc::new)
                    .into_iter()
                    .collect()
            } else if let Some(parent_name) = resolve_class_keyword(class, current_class) {
                // parent — resolve via all_classes first, then class_loader
                if let Some(cls) = find_class_by_name(all_classes, &parent_name) {
                    vec![Arc::clone(cls)]
                } else {
                    class_loader(&parent_name).into_iter().collect()
                }
            } else {
                // `Foo::` is a source-level reference: PHP resolves an
                // unqualified name against the current namespace before
                // the global scope, so a same-namespace class must win
                // over a global class of the same short name.
                let ns = current_class.and_then(|c| c.file_namespace.as_deref());
                let fqn =
                    crate::util::resolve_source_class_name(class, ns, all_classes, class_loader);
                class_loader(&fqn).into_iter().collect()
            };

            // When the member is a static property (starts with `$`),
            // resolve to the property's declared type instead of the
            // owning class.  This makes `self::$instance->method()`
            // resolve `method()` on the property's type, not on the
            // class that declares the static property.
            if let Some(prop_name) = member.strip_prefix('$') {
                let mut results: Vec<ResolvedType> = Vec::new();
                for cls in &owner_classes {
                    let resolved = super::type_resolution::resolve_property_types(
                        prop_name,
                        cls,
                        all_classes,
                        class_loader,
                    );
                    ResolvedType::extend_unique(
                        &mut results,
                        resolved.into_iter().map(ResolvedType::from_arc).collect(),
                    );
                }
                if !results.is_empty() {
                    return results;
                }
            } else {
                // Otherwise the member is an enum case or class constant.
                // Resolve it through the same constant-type path the call
                // resolver uses (`resolve_static_access_type`) instead of
                // returning the owning class outright: a typed or
                // inferred class constant holds whatever type its value
                // is (e.g. an enum case held in a `const Kind TYPED`),
                // not the type of the class that declares the constant.
                // An enum case's own class already resolves to itself
                // through this same path, so no separate case is needed.
                let mut const_results: Vec<ResolvedType> = Vec::new();
                for owner in &owner_classes {
                    let text = format!("{}::{member}", owner.fqn());
                    if let Some(ty) = super::call_resolution::resolve_static_access_type(&text, ctx)
                    {
                        let owning_class_name = owner.fqn().to_string();
                        let resolved = super::type_resolution::type_hint_to_classes_typed(
                            &ty,
                            &owning_class_name,
                            all_classes,
                            class_loader,
                        );
                        ResolvedType::extend_unique(
                            &mut const_results,
                            resolved.into_iter().map(ResolvedType::from_arc).collect(),
                        );
                    }
                }
                if !const_results.is_empty() {
                    return const_results;
                }
            }

            owner_classes
                .into_iter()
                .map(ResolvedType::from_arc)
                .collect()
        }

        // ── A bare class name, and `new ClassName` without trailing
        //    call parens, are the same reference written two ways ──
        SubjectExpr::ClassName(name) | SubjectExpr::NewExpr { class_name: name } => {
            // Both are source-level references: PHP resolves an
            // unqualified name against the current namespace before the
            // global scope, so a same-namespace class must win over a
            // global stub of the same short name.
            let ns = current_class.and_then(|c| c.file_namespace.as_deref());
            let fqn = crate::util::resolve_source_class_name(name, ns, all_classes, class_loader);
            class_loader(&fqn)
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect()
        }

        // ── Call expression ─────────────────────────────────────
        SubjectExpr::CallExpr { callee, args_text } => {
            // Narrowing on the call itself (`if ($h->get() instanceof
            // Foo) { $h->get()->m(); }`) has already been consulted by the
            // spine walk in `resolve_target_classes_expr`, which reads it
            // before resolving anything below the call.
            let mut hint: Option<PhpType> = None;
            let classes = Backend::resolve_call_return_types_on_receiver(
                callee,
                args_text,
                receiver,
                ctx,
                Some(&mut hint),
            );

            // Use the raw return type hint only when it actually carries
            // generic args a resolved class can benefit from: either the
            // class declares its own `@template` params, or (a `static<...>`
            // rebind on a class that only fixes its generics via
            // `@extends`) the hint's generic base names one of the
            // resolved classes directly, so the args are its own rebind
            // rather than a leftover from an unrelated wrapper type.  An
            // intersection hint (e.g. a conditional return type's winning
            // branch, `Foo&MockInterface`) is used unconditionally: without
            // it, `classes` is a flat, untagged list of the intersection's
            // members, which `types_joined` cannot tell apart from a union
            // of alternatives.
            if let Some(h) = hint {
                let hint_names_a_class = matches!(h.kind(), TypeKind::Generic(g) if classes
                    .iter()
                    .any(|c| g.name.as_str() == c.fqn().as_str() || g.name.as_str() == c.name.as_str()));
                let is_intersection = matches!(h.kind(), TypeKind::Intersection(_));
                if is_intersection
                    || classes.iter().any(|c| !c.template_params.is_empty())
                    || hint_names_a_class
                {
                    return ResolvedType::from_classes_with_hint(classes, h);
                }
                // A scalar conditional branch (and the `object`/`?object`
                // escape hatch) has no concrete class to carry it. Surface
                // it as a type-string-only candidate instead of dropping it,
                // so every consumer sees the branch selected at this call
                // site rather than re-reading the callable's broader union.
                if classes.is_empty() && (h.is_object() || h.all_members_primitive_scalar()) {
                    return vec![ResolvedType::from_type_string(h)];
                }
            }

            classes.into_iter().map(ResolvedType::from_arc).collect()
        }

        // ── Property chain ──────────────────────────────────────
        SubjectExpr::PropertyChain { base, property } => {
            // A type the forward walker narrowed this exact property path
            // to has already been consulted by the spine walk in
            // `resolve_target_classes_expr`, which reads it before
            // resolving anything below the property.
            let base_arcs = resolved_to_arcs(
                receiver.unwrap_or_else(|| resolve_target_classes_expr(base, access_kind, ctx)),
            );
            let mut arc_results: Vec<Arc<ClassInfo>> = Vec::new();
            // The generic members of the property's hint, e.g.
            // `Collection<int, Post>`.  The classes resolved from it carry
            // only its base name, so each is given its generic spelling
            // back below.
            let mut generic_hints: Vec<PhpType> = Vec::new();
            for cls in &base_arcs {
                let Some(hint) = resolve_property_type_hint(cls, property, class_loader) else {
                    continue;
                };
                let resolved = super::type_resolution::type_hint_to_classes_typed(
                    &hint,
                    &cls.fqn(),
                    all_classes,
                    class_loader,
                );
                generic_hints.extend(
                    hint.unwrap_nullable()
                        .union_members()
                        .into_iter()
                        .filter(|member| matches!(member.kind(), TypeKind::Generic(_)))
                        .cloned(),
                );

                ClassInfo::extend_unique_arc(&mut arc_results, resolved);
            }

            // ── `class-string<T>` unwrapping for `prop::` access ────
            // A property typed `class-string<T>` holds a class name at
            // runtime, not an instance of its own declared type.
            // `resolve_property_types` skips it (it isn't a class
            // type), so when the chain is followed by `::`, resolve
            // static members against `T` instead of leaving
            // `arc_results` empty.
            if arc_results.is_empty() && access_kind == AccessKind::DoubleColon {
                for cls in &base_arcs {
                    if let Some(raw_type) = resolve_property_type_hint(cls, property, class_loader)
                    {
                        let classes = resolve_class_string_inner_classes(
                            &raw_type,
                            &cls.fqn(),
                            all_classes,
                            class_loader,
                        );
                        ClassInfo::extend_unique_arc(&mut arc_results, classes);
                    }
                }
            }

            // The generic spelling of each class the hint named.
            let mut resolved = ResolvedType::from_classes(arc_results);
            for rt in &mut resolved {
                let Some(class) = &rt.class_info else {
                    continue;
                };
                let fqn = class.fqn();
                if let Some(hint) = generic_hints.iter().find(|hint| {
                    hint.base_name().is_some_and(|base| {
                        base == fqn.as_str() || crate::util::short_name(&fqn) == base
                    })
                }) {
                    rt.type_string = hint.clone();
                }
            }
            resolved
        }

        // ── Array access on variable or call expression ─────────
        SubjectExpr::ArrayAccess { base, segments } => {
            // Build the scope key using the canonical double-quote
            // format that the forward walker's `expr_to_subject_key`
            // produces (e.g. `$row["page"]`, `$stmts["0"]`).  Integer
            // indices are stringified because PHP normalises them, so
            // `$a[0]` and `$a["0"]` narrow the same subject.
            let scope_key = subject_scope_key(expr);

            // Check if the forward-walker scope has a narrowed type for
            // this array access (e.g. `$row['page']` narrowed via
            // `instanceof`, or `$stmts[0]` after a guard clause).
            if let Some(narrowed) = lookup_scope_for_subject(&scope_key, ctx) {
                return narrowed;
            }

            // When no scope resolver is available (top-level completion),
            // try resolving the full array access key through the forward
            // walker.  This picks up instanceof narrowing on array elements
            // (e.g. `$row['page'] instanceof Page` narrows `$row["page"]`).
            if ctx.scope_var_resolver.is_none() && matches!(base.as_ref(), SubjectExpr::Variable(_))
            {
                let dummy_class;
                let effective_class = match current_class {
                    Some(cc) => cc,
                    None => {
                        dummy_class = crate::class_lookup::class_context_placeholder(
                            ctx.content,
                            ctx.cursor_offset,
                        );
                        &dummy_class
                    }
                };
                let resolved = crate::type_engine::variable::resolution::resolve_variable_types(
                    &scope_key,
                    effective_class,
                    all_classes,
                    ctx.content,
                    ctx.cursor_offset,
                    class_loader,
                    ctx.backend,
                    Loaders::with_function(ctx.function_loader),
                );
                if !resolved.is_empty() {
                    return resolved;
                }
            }

            // When the base is a call expression (e.g. `$c->items()[0]`),
            // resolve the call's raw return type and use it as a candidate
            // for array-segment walking.  This mirrors the variable path
            // but sources the raw type from the method/function signature
            // instead of from docblock annotations or assignments.
            if let SubjectExpr::CallExpr { callee, args_text } = base.as_ref() {
                // Resolve the call's return type with template and generic
                // substitution applied, so that a method declared
                // `@return T[]` with a `class-string<T>` parameter resolves
                // its element type from the call-site argument (e.g.
                // `$a->findChildrenOfType(Foo::class)[0]` → `Foo`).  The
                // un-substituted raw return type is kept as a fallback for
                // callees the hint path doesn't cover.
                let mut hint: Option<PhpType> = None;
                let _ = Backend::resolve_call_return_types_expr_with_hint(
                    callee,
                    args_text,
                    ctx,
                    Some(&mut hint),
                );
                let raw = resolve_call_raw_return_type(callee, args_text, ctx);
                let candidates = hint.into_iter().chain(raw);
                if let Some(resolved) =
                    crate::completion::source::helpers::try_chained_array_access_with_candidates(
                        candidates,
                        segments,
                        current_class,
                        all_classes,
                        class_loader,
                    )
                {
                    return resolved_array_access_type_to_resolved(
                        resolved,
                        current_class,
                        all_classes,
                        class_loader,
                    );
                }
                // Neither the substituted hint nor the raw return type had
                // array-shape / generic / iterable annotations covering the
                // bracket access.  Return empty: `call()[i]` is never the
                // same type as `call()`.
                return vec![];
            }

            let base_var = base.to_subject_text();

            // Build candidate raw types from multiple strategies.
            // Each is tried as a complete pipeline (raw type →
            // segment walk → ClassInfo); the first that succeeds
            // through all segments wins.

            // ── Property chain raw type ─────────────────────────
            // When the base is a property chain (e.g. `$this->cache`,
            // `$obj->items`), resolve the owning class and extract
            // the property's raw type hint.  This preserves generic
            // parameters like `array<string, IntCollection>` or
            // `Collection<int, Translation>` that would be lost if
            // we resolved through `type_hint_to_classes_typed` first.
            let property_raw_type: Option<PhpType> = if let SubjectExpr::PropertyChain {
                base: prop_base,
                property,
            } = base.as_ref()
            {
                let owner_arcs =
                    resolved_to_arcs(resolve_target_classes_expr(prop_base, access_kind, ctx));
                owner_arcs.iter().find_map(|cls| {
                    crate::inheritance::resolve_property_type_hint(cls, property, class_loader)
                })
            } else {
                None
            };

            let docblock_type: Option<PhpType> = docblock::find_iterable_raw_type_in_source(
                ctx.content,
                ctx.cursor_offset as usize,
                &base_var,
            )
            .map(|t| crate::util::resolve_php_type_names(&t, ctx.class_loader));
            // resolve_variable_types is designed for bare `$variable` names;
            // property chains like `$this->query->joins` are handled by the
            // property_raw_type strategy above.  Skip this strategy for
            // non-variable expressions (chains, array access, comparisons,
            // null coalescing, boolean expressions) to avoid polluting
            // the scope cache with unsupported keys.
            let ast_type: Option<PhpType> = if is_bare_variable(&base_var) {
                // When a scope_var_resolver is available (i.e. we are
                // inside the forward walker), read the variable type
                // from the in-progress ScopeState instead of calling
                // resolve_variable_types which would re-enter the
                // forward walker and cause stack overflow.
                if let Some(scope_resolver) = ctx.scope_var_resolver {
                    let prefixed = if base_var.starts_with('$') {
                        base_var.clone()
                    } else {
                        format!("${}", base_var)
                    };
                    let from_scope = scope_resolver(&prefixed);
                    if from_scope.is_empty() {
                        None
                    } else {
                        Some(ResolvedType::types_joined(&from_scope))
                    }
                } else {
                    let dummy_class;
                    let effective_class = match current_class {
                        Some(cc) => cc,
                        None => {
                            dummy_class = crate::class_lookup::class_context_placeholder(
                                ctx.content,
                                ctx.cursor_offset,
                            );
                            &dummy_class
                        }
                    };
                    let resolved = crate::type_engine::variable::resolution::resolve_variable_types(
                        &base_var,
                        effective_class,
                        all_classes,
                        ctx.content,
                        ctx.cursor_offset,
                        class_loader,
                        ctx.backend,
                        Loaders::with_function(ctx.function_loader),
                    );
                    if resolved.is_empty() {
                        None
                    } else {
                        Some(ResolvedType::types_joined(&resolved))
                    }
                }
            } else {
                None
            };

            // The annotation goes first because the walker can lose its
            // generics on the way through, but a walker type that refines
            // the annotation is the annotation read further (an `@var
            // array<T>` whose `T` it knows the bound of, a shape a guard
            // has narrowed) and describes the value better.
            let (first, second) = match (docblock_type, ast_type) {
                (Some(doc), Some(ast)) if ast != doc && ast.is_subtype_of(&doc) => {
                    (Some(ast), Some(doc))
                }
                (doc, ast) => (doc, ast),
            };
            let candidates = property_raw_type.into_iter().chain(first).chain(second);

            if let Some(resolved) =
                crate::completion::source::helpers::try_chained_array_access_with_candidates(
                    candidates,
                    segments,
                    current_class,
                    all_classes,
                    class_loader,
                )
            {
                return resolved_array_access_type_to_resolved(
                    resolved,
                    current_class,
                    all_classes,
                    class_loader,
                );
            }
            // Segment walk failed — the base type does not have
            // array-shape, generic, or iterable annotations that
            // cover bracket access.  Return empty: `$var['key']` is
            // never the same type as `$var`.
            vec![]
        }

        // ── Bare variable ───────────────────────────────────────
        SubjectExpr::Variable(var_name) => resolve_variable_fallback(var_name, access_kind, ctx),

        // ── Callee-only variants (MethodCall, StaticMethodCall,
        //    FunctionCall) should not appear as top-level subjects;
        //    they are wrapped in CallExpr.  If they do appear
        //    (e.g. from a partial parse), treat as class name. ────
        SubjectExpr::MethodCall { .. }
        | SubjectExpr::StaticMethodCall { .. }
        | SubjectExpr::FunctionCall(_) => {
            let text = expr.to_subject_text();
            if let Some(cls) = find_class_by_name(all_classes, &text) {
                return vec![ResolvedType::from_arc(Arc::clone(cls))];
            }
            class_loader(&text)
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect()
        }
    }
}

/// Package a segment-walked array access type as `ResolvedType`s.
///
/// The walk in [`crate::completion::source::helpers::try_chained_array_access_with_candidates`]
/// answers with whatever type the bracket access resolves to — a class,
/// a scalar (`array{message: string}['message']` → `string`), or
/// anything else. When it names classes, those carry the full type
/// string as a hint; when it doesn't (a scalar, or a shape/generic type
/// with no matching class), the type string alone is preserved so
/// downstream consumers (hover, hint-based hover fallbacks, template
/// binding) still see it rather than nothing at all.
fn resolved_array_access_type_to_resolved(
    resolved: PhpType,
    current_class: Option<&ClassInfo>,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<ResolvedType> {
    let current_class_name = current_class.map(|c| c.name.as_str()).unwrap_or("");
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &resolved,
        current_class_name,
        all_classes,
        class_loader,
    );
    if classes.is_empty() {
        vec![ResolvedType::from_type_string(resolved)]
    } else {
        ResolvedType::from_classes_with_hint(classes, resolved)
    }
}

/// Extract the raw return type string from a call expression's callee.
///
/// Given a `CallExpr`'s callee and arguments, resolves the owning class
/// (for method/static-method calls) or the function info (for standalone
/// functions), finds the matching method/function, and returns its raw
/// return type string (e.g. `"Item[]"`).  This is used by the
/// `ArrayAccess` handler to strip array dimensions and resolve the
/// element type when the base of `[0]` is a call expression.
fn resolve_call_raw_return_type(
    callee: &SubjectExpr,
    _args_text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    match callee {
        SubjectExpr::MethodCall { base, method } => {
            let base_classes =
                resolved_to_arcs(resolve_target_classes_expr(base, AccessKind::Arrow, ctx));
            raw_return_type_with_magic_fallback(&base_classes, method, "__call", ctx)
        }
        SubjectExpr::StaticMethodCall { class, method } => {
            let owner = resolve_static_owner_class(class, ctx);
            let owner = owner.as_slice();
            raw_return_type_with_magic_fallback(owner, method, "__callStatic", ctx)
        }
        SubjectExpr::FunctionCall(fn_name) => {
            if let Some(fl) = ctx.function_loader
                && let Some(func_info) = fl(fn_name, 0)
            {
                return func_info.return_type.clone();
            }
            None
        }
        _ => None,
    }
}

/// The raw return type of `method` on the first of `classes` that
/// declares it, falling back to `magic_name` (`__call`/`__callStatic`)
/// when the method is virtual (from a `@method` tag or `@mixin`) and
/// carries no return type of its own, or is not declared at all.
///
/// A *real* method with no return type does not fall through: it is
/// invoked directly at runtime, never through the magic method, so
/// reading the magic method's return type there would be answering a
/// different call. `classes` is a slice rather than one class because
/// `$obj->method()` on a union receiver tries each alternative in turn.
fn raw_return_type_with_magic_fallback(
    classes: &[Arc<ClassInfo>],
    method: &str,
    magic_name: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    for cls in classes {
        // Use a fully-resolved class so that inherited docblock return
        // types (e.g. `list<Pen>` from an interface or parent) are
        // visible instead of the bare native hint.
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        if let Some(m) = merged.get_method_ci(method) {
            if let Some(ref ret) = m.return_type {
                return Some(ret.clone());
            }
            if !m.is_virtual {
                continue;
            }
        }
        if let Some(m) = merged.get_method_ci(magic_name)
            && let Some(ref ret) = m.return_type
        {
            return Some(ret.clone());
        }
    }
    None
}

// ─── Enriched subject resolution for diagnostics ────────────────────────────

/// Whether every non-null member of `ty` is the bare `string` type.
///
/// `$var::method()` is valid PHP when `$var` holds a class name at
/// runtime — the string names the class — so a `string`-only subject
/// accessed via `::` can only be "unverifiable", never a scalar-access
/// error the way `$var->method()` on a scalar is.  Other scalars
/// (`int`, `bool`, …) can never be a class name, so they keep reporting
/// as scalar access.
fn is_all_string_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => name.eq_ignore_ascii_case("string"),
        TypeKind::Nullable(inner) => is_all_string_type(inner),
        TypeKind::Union(members) => members
            .iter()
            .filter(|m| !m.is_null())
            .all(is_all_string_type),
        _ => false,
    }
}

/// The outcome of resolving a subject for diagnostic purposes.
///
/// [`resolve_target_classes`] only returns `Vec<Arc<ClassInfo>>` and
/// silently drops scalar types and type-string-only entries.
/// Diagnostics need to know *why* resolution returned empty — was the
/// subject a scalar type (runtime crash), an unresolvable class name
/// (likely typo / missing import), or truly untyped?  This enum
/// carries that distinction so the diagnostic collector can emit the
/// right message and severity.
///
/// ## Architectural invariant
///
/// Every `SubjectOutcome` **must** be derived from the same resolution
/// pass that completion and hover use.  Re-resolving a variable
/// through a secondary helper (e.g. `resolve_variable_type`)
/// bypasses narrowing (instanceof, assert, ternary, `&&`) and
/// produces false positives.  See [`resolve_subject_outcome`] for
/// how this is enforced for each subject variant.
#[derive(Clone, Debug)]
pub(crate) enum SubjectOutcome {
    /// Subject resolved to one or more classes.
    Resolved(Vec<Arc<ClassInfo>>),
    /// Subject resolved to a scalar type — member access is always a
    /// runtime crash.  The `PhpType` is the resolved scalar type
    /// (e.g. `int`, `string`, `bool|int`) with null stripped.
    Scalar(PhpType),
    /// Subject resolved to a class name that couldn't be loaded.
    UnresolvableClass(PhpType),
    /// Subject resolved to `mixed`.
    ///
    /// Distinct from [`Untyped`](Self::Untyped): `mixed` is an answer, not
    /// the absence of one. It is what an undocblocked `array`'s elements,
    /// a `@return mixed` accessor and a `mixed` parameter all hold, so the
    /// gap is in what the code says about the value rather than in what
    /// the type engine could work out. A member on it is unverifiable
    /// either way, but the two are worth telling apart: one is a note
    /// about the codebase's annotations, the other a report of our own
    /// resolution falling short.
    Mixed,
    /// Subject type could not be resolved — no class information
    /// available.
    Untyped,
}

/// Resolve a subject to a [`SubjectOutcome`] in a single pass.
///
/// This is the unified entry point for diagnostic subject resolution.
/// It resolves the subject to `Vec<ResolvedType>` (the same pipeline
/// used by completion and hover) and classifies the result:
///
///   - If any entry has `class_info`, return `Resolved`.
///   - If all entries are primitive scalars, return `Scalar`.
///   - If a type string refers to an unloadable class, return
///     `UnresolvableClass`.
///   - If the result is empty, return `Untyped`.
pub(crate) fn resolve_subject_outcome(
    subject: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> SubjectOutcome {
    let resolved = resolve_target_classes(subject, access_kind, ctx);
    if !resolved.is_empty() {
        // ── Check for class-bearing entries ──────────────────────
        let arced: Vec<Arc<ClassInfo>> = ResolvedType::into_arced_classes(resolved.clone());
        if !arced.is_empty() {
            return SubjectOutcome::Resolved(arced);
        }

        // ── All entries are type-string-only (no class info) ────
        let joined = ResolvedType::types_joined(&resolved);

        // `mixed` is a resolved type, and one that absorbs whatever stands
        // beside it, so it is answered before the narrower classifications
        // below get a chance to describe only part of the value.
        if joined.contains_mixed() {
            return SubjectOutcome::Mixed;
        }

        // Pure scalar — member access is a runtime crash, unless this
        // is a `::` access on a `string`-only subject (see
        // `is_all_string_type`), which is unverifiable rather than
        // wrong.
        if joined.all_members_primitive_scalar() {
            let scalar = joined.non_null_type().unwrap_or_else(|| joined.clone());
            if access_kind != AccessKind::DoubleColon || !is_all_string_type(&scalar) {
                return SubjectOutcome::Scalar(scalar);
            }
        }

        // stdClass / object — synthetic resolution.
        if resolved
            .iter()
            .any(|rt| rt.type_string.is_named_ci("stdclass") || rt.type_string.is_object())
        {
            let synthetic = Arc::new(ClassInfo {
                name: crate::atom::atom("stdClass"),
                ..ClassInfo::default()
            });
            return SubjectOutcome::Resolved(vec![synthetic]);
        }

        // Non-scalar, non-class type — check for unresolvable class.
        if let Some(unresolved) = check_unresolvable_class_name(&joined, ctx.class_loader) {
            return SubjectOutcome::UnresolvableClass(unresolved);
        }
        return SubjectOutcome::Untyped;
    }

    // ── Result is empty — classify why ──────────────────────────
    let expr = SubjectExpr::parse(subject);

    // For call expressions, check the raw return type hint.
    if let SubjectExpr::CallExpr {
        callee,
        args_text: _,
    } = &expr
    {
        if let Some(scalar) = resolve_call_scalar_return(callee, access_kind, ctx) {
            return SubjectOutcome::Scalar(scalar);
        }
        // A declared `mixed` is why this resolved to no class, and saying
        // so is the difference between reporting the codebase's missing
        // annotation and reporting our own resolution falling short.
        if call_returns_mixed(callee, access_kind, ctx) {
            return SubjectOutcome::Mixed;
        }
        // Note: a call returning `object`/`?object` is already caught by
        // the "stdClass / object" check above — `resolve_target_classes`
        // surfaces it as a type-string-only candidate instead of an empty
        // result, so `resolved` would not have been empty in that case.
        // Try unresolvable class detection for function calls.
        if let SubjectExpr::FunctionCall(fn_name) = callee.as_ref()
            && let Some(fl) = ctx.function_loader
            && let Some(func_info) = fl(fn_name.as_str(), 0)
            && let Some(ref raw_type) = func_info.return_type
            && let Some(unresolved) = check_unresolvable_class_name(raw_type, ctx.class_loader)
        {
            return SubjectOutcome::UnresolvableClass(unresolved);
        }
    }

    // For property chains, check the property's type hint.
    if let SubjectExpr::PropertyChain { base, property } = &expr {
        let base_arcs = resolved_to_arcs(resolve_target_classes_expr(base, access_kind, ctx));
        for cls in &base_arcs {
            let merged =
                resolve_class_fully_maybe_cached(cls, ctx.class_loader, ctx.resolved_class_cache);
            if let Some(parsed) = resolve_property_type_hint(&merged, property, ctx.class_loader) {
                if parsed.all_members_primitive_scalar() {
                    let scalar = parsed.non_null_type().unwrap_or(parsed);
                    if access_kind != AccessKind::DoubleColon || !is_all_string_type(&scalar) {
                        return SubjectOutcome::Scalar(scalar);
                    }
                    return SubjectOutcome::Untyped;
                }
                return SubjectOutcome::Untyped;
            }
        }
    }

    // For bare variables, try the hover fallback for UnresolvableClass
    // detection only.
    if let SubjectExpr::Variable(var_name) = &expr
        && let Some(resolved_type) =
            crate::type_engine::variable::resolution::resolve_variable_php_type(
                var_name,
                ctx.content,
                ctx.cursor_offset,
                ctx.current_class,
                ctx.all_classes,
                ctx.class_loader,
                ctx.backend,
                Loaders::with_function(ctx.function_loader),
            )
        && let Some(unresolved) = check_unresolvable_class_name(&resolved_type, ctx.class_loader)
    {
        return SubjectOutcome::UnresolvableClass(unresolved);
    }

    SubjectOutcome::Untyped
}

/// Check whether a call expression's return type is a scalar.
///
/// Inspects the raw return type hint on the method or function without
/// going through the full class resolution pipeline.
fn resolve_call_scalar_return(
    callee: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let hint = declared_call_return_type(callee, access_kind, ctx, &|hint| {
        hint.all_members_primitive_scalar()
    })?;
    Some(hint.non_null_type().unwrap_or(hint))
}

/// Whether the method or function a call names declares `mixed`.
///
/// A call that resolves to no class at all has two very different causes
/// — the declaration said `mixed`, or nothing could be worked out — and
/// only the declaration is on record to tell them apart.
fn call_returns_mixed(
    callee: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> bool {
    declared_call_return_type(callee, access_kind, ctx, &PhpType::contains_mixed).is_some()
}

/// The return type declared on the method or function a call names, when
/// `accept` recognises it.
///
/// Reads the hint straight off the `MethodInfo`/`FunctionInfo` rather than
/// going through the full class resolution pipeline: the callers want to
/// know what the declaration says, not what a resolver can make of it.
/// `accept` is consulted per candidate so a receiver whose type is a union
/// keeps looking past a class that declares something else.
fn declared_call_return_type(
    callee: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
    accept: &dyn Fn(&PhpType) -> bool,
) -> Option<PhpType> {
    match callee {
        // Instance method call: $obj->getAge()
        SubjectExpr::MethodCall { base, method } => {
            let base_arcs = resolved_to_arcs(resolve_target_classes_expr(base, access_kind, ctx));
            declared_return_type_of(&base_arcs, method, ctx, accept)
        }
        // Standalone function call: getInt()
        SubjectExpr::FunctionCall(fn_name) => {
            if let Some(fl) = ctx.function_loader
                && let Some(func_info) = fl(fn_name, 0)
                && let Some(ref hint) = func_info.return_type
                && accept(hint)
            {
                return Some(hint.clone());
            }
            None
        }
        // Static method call: Foo::getInt()
        SubjectExpr::StaticMethodCall { class, method } => {
            let cls = (ctx.class_loader)(class);
            declared_return_type_of(cls.as_slice(), method, ctx, accept)
        }
        _ => None,
    }
}

/// The first declared return type `accept` recognises among `classes`'
/// `method`, or `None` when it names no such method on any of them.
///
/// `classes` is a slice rather than one class because `$obj->method()`
/// on a union receiver tries each alternative in turn; `accept` lets a
/// caller looking for a specific shape (all-scalar, `mixed`) keep
/// looking past a class whose declaration is something else.
fn declared_return_type_of(
    classes: &[Arc<ClassInfo>],
    method: &str,
    ctx: &ResolutionCtx<'_>,
    accept: &dyn Fn(&PhpType) -> bool,
) -> Option<PhpType> {
    for cls in classes {
        let resolved =
            resolve_class_fully_maybe_cached(cls, ctx.class_loader, ctx.resolved_class_cache);
        if let Some(m) = resolved.get_method_ci(method)
            && let Some(ref hint) = m.return_type
            && accept(hint)
        {
            return Some(hint.clone());
        }
    }
    None
}

/// Check whether a raw type string refers to a class that cannot be
/// loaded.
///
/// Returns `Some(class_name)` when the type looks like a class name
/// (not scalar, not a PHPDoc pseudo-type) but the class loader cannot
/// find it.  Returns `None` for scalars, unions, shapes, and types
/// that resolve successfully.
fn check_unresolvable_class_name(
    raw_type: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if raw_type.all_members_scalar() || raw_type.is_mixed() {
        return None;
    }

    let effective = raw_type.non_null_type().unwrap_or_else(|| raw_type.clone());
    let base = effective.base_name()?;

    if class_loader(base).is_none() {
        Some(PhpType::named(atom(base)))
    } else {
        None
    }
}

/// Resolve `$this` from the forward-walk scope when it carries a
/// narrowed or seeded type.
///
/// `$this` is normally resolved from the lexical `current_class`, but a
/// closure body may have a `$this` type that the enclosing class cannot
/// supply:
///
///   - `assert($this instanceof X)` inside a top-level closure (Pest
///     tests) seeds `$this` in the forward-walk scope even though there
///     is no lexical class.
///   - `instanceof` narrowing refines `$this` to a subclass.
///
/// This consults the injected `scope_var_resolver` first (used while the
/// forward walker resolves an assignment RHS), then the diagnostic scope
/// snapshot cache (used by the member-verification path after the scope
/// has been built), and finally runs the walker itself for the consumers
/// that have neither — hover, completion and go-to-definition each answer
/// a single request, so the one body walk `$this` costs there is the same
/// walk any other variable subject in the chain already pays for.
/// Returns `None` when nothing yields a type so the caller falls back to
/// `current_class`.
fn resolve_this_from_scope(ctx: &ResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    use crate::type_engine::variable::forward_walk;

    if let Some(scope_resolver) = ctx.scope_var_resolver {
        let from_scope = scope_resolver("$this");
        return (!from_scope.is_empty()).then_some(from_scope);
    }

    if forward_walk::is_diagnostic_scope_active()
        && !forward_walk::is_building_scopes()
        && forward_walk::scope_snapshots_cover(ctx.cursor_offset)
    {
        // The snapshot is authoritative here: the walker records `$this`
        // exactly when something narrowed or seeded it, so a miss means
        // there is no proof to find and re-walking would only repeat the
        // pass that built the snapshot, once per `$this->` site.  That
        // only holds where the walk actually went, which a targeted walk
        // limits to the bodies it was asked about.
        return forward_walk::lookup_diagnostic_scope("$this", ctx.cursor_offset)
            .filter(|types| !types.is_empty());
    }

    let dummy_class;
    let effective_class = match ctx.current_class {
        Some(cc) => cc,
        None => {
            dummy_class =
                crate::class_lookup::class_context_placeholder(ctx.content, ctx.cursor_offset);
            &dummy_class
        }
    };
    let resolved = crate::type_engine::variable::resolution::resolve_variable_types(
        "$this",
        effective_class,
        ctx.all_classes,
        ctx.content,
        ctx.cursor_offset,
        ctx.class_loader,
        ctx.backend,
        Loaders::with_function(ctx.function_loader),
    );
    (!resolved.is_empty()).then_some(resolved)
}

/// Builds a cache-key discriminator from the resolved types of the local
/// variables an expression references, so that two textually identical
/// chains resolve from separate cache entries when their variables hold
/// different types.
///
/// Returns `None` when variable types cannot be resolved cheaply (no
/// forward-walk scope resolver and no active diagnostic scope snapshot);
/// the caller then falls back to a per-site key.  Variables that resolve
/// to nothing contribute an empty type: an unresolvable receiver yields an
/// empty chain result regardless, so sharing those entries is safe.
fn scope_type_discriminator(vars: &[String], ctx: &ResolutionCtx<'_>) -> Option<String> {
    use crate::type_engine::variable::forward_walk;

    let scope_active =
        forward_walk::is_diagnostic_scope_active() && !forward_walk::is_building_scopes();
    if ctx.scope_var_resolver.is_none() && !scope_active {
        return None;
    }

    let mut names: Vec<&String> = vars.iter().collect();
    names.sort();
    names.dedup();

    let mut disc = String::new();
    for name in names {
        let resolved: Vec<ResolvedType> = if let Some(scope_resolver) = ctx.scope_var_resolver {
            scope_resolver(name)
        } else {
            forward_walk::lookup_diagnostic_scope(name, ctx.cursor_offset).unwrap_or_default()
        };

        let mut parts: Vec<String> = resolved
            .iter()
            .map(|rt| match &rt.class_info {
                Some(ci) => ci.fqn().to_string(),
                None => rt.type_string.to_string(),
            })
            .collect();
        parts.sort();
        parts.dedup();

        disc.push('|');
        disc.push_str(name);
        disc.push('=');
        disc.push_str(&parts.join("&"));
    }
    Some(disc)
}

/// Resolve the classes named by `class-string<T>` (or `?class-string<T>`,
/// or a union containing it) inside `ty`.
///
/// A subject typed `class-string<T>` holds a class name at runtime, not
/// an instance of `T` — but a `::` access on it (`$class::method()`)
/// resolves against `T`'s static members.  Used by both bare-variable
/// and property-chain subjects so `$var::` and `$obj->prop::` share the
/// same unwrapping behaviour.
fn resolve_class_string_inner_classes(
    ty: &PhpType,
    owning_class_name: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<Arc<ClassInfo>> {
    fn inner_types(ty: &PhpType) -> Vec<&PhpType> {
        match ty.kind() {
            TypeKind::ClassString(Some(inner)) => vec![inner],
            TypeKind::Nullable(inner) => inner_types(inner),
            TypeKind::Union(members) => members.iter().flat_map(inner_types).collect(),
            _ => vec![],
        }
    }

    // A literal string that names a class is as usable for `::` access as
    // `class-string<T>` is: PHP resolves the static call through whatever
    // class name the string holds at runtime, whether or not the analyser
    // wrapped it in `class-string`. This is what lets a foreach variable
    // bound to a `Foo::class` array element (a literal `'Foo'`, not a
    // `class-string<Foo>`, per PHPStan's own reading of such a constant)
    // still dispatch `$item::method()`.
    fn literal_string_class_names(ty: &PhpType) -> Vec<String> {
        match ty.kind() {
            TypeKind::Literal(literal) => match &**literal {
                crate::php_type::LiteralValue::String(raw) => {
                    crate::util::unescape_php_string_literal(raw)
                        .into_iter()
                        .collect()
                }
                _ => vec![],
            },
            TypeKind::Nullable(inner) => literal_string_class_names(inner),
            TypeKind::Union(members) => members
                .iter()
                .flat_map(literal_string_class_names)
                .collect(),
            _ => vec![],
        }
    }

    let mut results = Vec::new();
    for inner in inner_types(ty) {
        let resolved = super::type_resolution::type_hint_to_classes_typed(
            inner,
            owning_class_name,
            all_classes,
            class_loader,
        );
        ClassInfo::extend_unique_arc(&mut results, resolved);
    }
    for name in literal_string_class_names(ty) {
        if let Some(cls) = class_loader(&name) {
            ClassInfo::extend_unique_arc(&mut results, vec![cls]);
        }
    }
    results
}

/// A member name written as `$variable` — a dynamic method/property
/// access such as `$obj->$name()`, `Class::$name()`, or their first-class
/// callable forms — resolves through whatever literal string that
/// variable holds at the call site: `$name = 'length'; $obj->$name()`
/// calls `length()` exactly as `$obj->length()` would.
///
/// Returns `member` unchanged when it is not a bare `$variable`
/// reference, or when the variable's resolved type carries no known
/// literal string value (the caller then keeps its normal, currently
/// unresolvable, behaviour for a truly dynamic name).
pub(crate) fn resolve_dynamic_member_name<'a>(
    member: &'a str,
    ctx: &ResolutionCtx<'_>,
) -> std::borrow::Cow<'a, str> {
    if !member.starts_with('$') || !is_bare_variable(member) {
        return std::borrow::Cow::Borrowed(member);
    }
    resolve_variable_fallback(member, AccessKind::Arrow, ctx)
        .into_iter()
        .find_map(|rt| match rt.type_string.kind() {
            TypeKind::Literal(literal) => match &**literal {
                crate::php_type::LiteralValue::String(raw) => {
                    crate::util::unescape_php_string_literal(raw)
                }
                _ => None,
            },
            _ => None,
        })
        .map(std::borrow::Cow::Owned)
        .unwrap_or(std::borrow::Cow::Borrowed(member))
}

/// Resolve a bare `$var` subject to its classes.
///
/// Resolves a variable to its classes by running the full variable
/// resolution pipeline (including narrowing from instanceof, assert,
/// ternary, and `&&` chains) and converting the result to
/// `Vec<Arc<ClassInfo>>` (dropping type-string-only entries).
fn resolve_variable_fallback(
    var_name: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.all_classes;
    let class_loader = ctx.class_loader;
    let function_loader = ctx.function_loader;

    let dummy_class;
    let effective_class = match current_class {
        Some(cc) => cc,
        None => {
            dummy_class =
                crate::class_lookup::class_context_placeholder(ctx.content, ctx.cursor_offset);
            &dummy_class
        }
    };

    // ── `$var::` where `$var` holds a class-string ──
    if access_kind == AccessKind::DoubleColon {
        let class_string_targets =
            crate::type_engine::variable::class_string_resolution::resolve_class_string_targets(
                var_name,
                effective_class,
                all_classes,
                ctx.content,
                ctx.cursor_offset,
                class_loader,
                ctx.backend,
            );
        if !class_string_targets.is_empty() {
            return class_string_targets
                .into_iter()
                .map(ResolvedType::from_class)
                .collect();
        }
    }

    // Guard: resolve_variable_types is designed for bare `$variable`
    // names.  SubjectExpr::Variable can carry complex expressions
    // (array access like `$arr['key']`, null coalescing, comparisons)
    // that will never match a scope entry.  Skip them to avoid wasted
    // backward scans and fallthrough noise.
    let bare_variable = is_bare_variable(var_name);
    let resolved_types = if bare_variable {
        // When a scope variable resolver is available (i.e. we are
        // inside the forward walker's scope-building pass), read the
        // variable's type directly from the in-progress ScopeState.
        // This avoids calling resolve_variable_types which would
        // trigger a full forward walk of the method body for every
        // variable access — an O(N²) blowup on files with closures.
        if let Some(scope_resolver) = ctx.scope_var_resolver {
            let prefixed = if var_name.starts_with('$') {
                var_name.to_string()
            } else {
                format!("${}", var_name)
            };
            scope_resolver(&prefixed)
        } else {
            super::variable::resolution::resolve_variable_types(
                var_name,
                effective_class,
                all_classes,
                ctx.content,
                ctx.cursor_offset,
                class_loader,
                ctx.backend,
                Loaders::with_function(function_loader),
            )
        }
    } else {
        vec![]
    };

    // ── @var docblock fallback ───────────────────────────────────
    // When the statement walk found no assignments for this variable,
    // check for a standalone `/** @var Type $var */` annotation above
    // the cursor.  This handles Blade templates and files where the
    // only type source is a docblock assertion.
    let resolved_types = if resolved_types.is_empty() && bare_variable {
        let prefixed = if var_name.starts_with('$') {
            var_name.to_string()
        } else {
            format!("${}", var_name)
        };
        if let Some(var_type) = crate::docblock::find_var_raw_type_in_source(
            ctx.content,
            ctx.cursor_offset as usize,
            &prefixed,
        ) {
            let classes = super::type_resolution::type_hint_to_classes_typed(
                &var_type,
                &effective_class.name,
                all_classes,
                class_loader,
            );
            classes.into_iter().map(ResolvedType::from_arc).collect()
        } else {
            vec![]
        }
    } else {
        resolved_types
    };

    // ── `class-string<T>` unwrapping for `$var::` access ────────
    // When the variable's type is `class-string<T>` (e.g. from a
    // `@param class-string<BackedEnum> $class` annotation) and the
    // access kind is `::`, unwrap the inner type `T` and resolve it
    // to classes so that static members are offered against `T`.
    if access_kind == AccessKind::DoubleColon {
        let mut class_string_results: Vec<ResolvedType> = Vec::new();
        for rt in &resolved_types {
            let classes = resolve_class_string_inner_classes(
                &rt.type_string,
                &effective_class.name,
                all_classes,
                class_loader,
            );
            for cls in classes {
                ResolvedType::push_unique(&mut class_string_results, ResolvedType::from_arc(cls));
            }
        }
        if !class_string_results.is_empty() {
            return class_string_results;
        }
    }

    resolved_types
}

// ── Static owner class resolution ───────────────────────────────────

/// Resolve the class that a bare `self`/`static` keyword refers to at
/// the cursor position.
///
/// Normally this is the lexically enclosing class, but inside a
/// closure whose enclosing call site declares `@param-closure-this`
/// (e.g. a Laravel `Macroable` or Carbon `macro()` registration), the
/// runtime binds the closure with the target class as its scope
/// (`Closure::bind`), so `self::` and `static::` refer to the bound
/// target rather than the class that lexically encloses the closure.
fn resolve_self_static_class(ctx: &ResolutionCtx<'_>) -> Option<ClassInfo> {
    super::variable::closure_resolution::find_closure_this_override(ctx)
        .or_else(|| ctx.current_class.cloned())
}

/// Resolve a static class reference (`self`, `static`, `parent`, or a
/// class name) to its `ClassInfo`.
///
/// Handles the `self`/`static`/`parent` keywords and falls back to
/// `class_loader` then `resolve_target_classes` for named classes.
pub(in crate::type_engine) fn resolve_static_owner_class(
    class: &str,
    rctx: &ResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    if is_self_or_static(class) {
        resolve_self_static_class(rctx).map(Arc::new)
    } else if let Some(resolved_name) = resolve_class_keyword(class, rctx.current_class) {
        // parent — load via class_loader so we get the full parent ClassInfo
        (rctx.class_loader)(&resolved_name)
    } else {
        let ns = rctx.current_class.and_then(|c| c.file_namespace.as_deref());
        let fqn =
            crate::util::resolve_source_class_name(class, ns, rctx.all_classes, rctx.class_loader);
        (rctx.class_loader)(&fqn).or_else(|| {
            resolved_to_arcs(resolve_target_classes(
                class,
                crate::AccessKind::DoubleColon,
                rctx,
            ))
            .into_iter()
            .next()
        })
    }
}

/// Whether a subject expression transitively roots in a variable
/// (`$var`, `$this`, `self`, `static`, or `parent`), possibly through
/// array accesses or nested property chains.
///
/// Such expressions are subject to `instanceof`/`assert` narrowing that
/// changes their resolved type at different positions in the same method
/// body (e.g. `$args[0]->value instanceof Foo` in one `if` branch vs.
/// `instanceof Bar` in the following `elseif`).  Their resolution must
/// therefore never be cached by subject text alone.  Expressions rooted
/// in a call (`$this->make()->prop`) resolve deterministically and stay
/// cacheable.
fn base_roots_in_variable(expr: &SubjectExpr) -> bool {
    let mut node = expr;
    loop {
        node = match node {
            SubjectExpr::This
            | SubjectExpr::SelfKw
            | SubjectExpr::StaticKw
            | SubjectExpr::Parent
            | SubjectExpr::Variable(_) => return true,
            SubjectExpr::PropertyChain { base, .. } | SubjectExpr::ArrayAccess { base, .. } => base,
            _ => return false,
        };
    }
}

/// Whether `expr` is a call that narrowing can key on: an argument-less
/// instance method call whose receiver roots in a variable (`$job->data()`,
/// `$this->getKernel()`, `$e->getExpr()->getExpr()`).  Everything else,
/// calls with arguments, function calls, static calls, resolves purely
/// from its signature and is never a narrowing subject.  The key itself is
/// [`subject_scope_key`], spelled exactly as the AST side spells it.
pub(crate) fn is_narrowable_call(expr: &SubjectExpr) -> bool {
    let SubjectExpr::CallExpr { callee, args_text } = expr else {
        return false;
    };
    args_text.trim().is_empty()
        && matches!(callee.as_ref(), SubjectExpr::MethodCall { base, .. } if base.scope_key_roots_in_variable())
}

/// Build the canonical forward-walker scope key for a subject
/// expression (e.g. `$row["page"]`, `$stmts["0"]`, `$args["0"]->value`).
///
/// Mirrors the format that `expr_to_subject_key` produces on the AST
/// side: property paths join with `->`, array keys use double quotes,
/// and integer indices are stringified so `$a[0]` and `$a["0"]` map to
/// the same key (matching PHP's integer/string key coercion).  Any
/// subject shape the forward walker does not key on falls back to
/// `to_subject_text`.
/// Walks the spine iteratively: a property or array-access chain nests one
/// node per link and there is no bound on how long a generated chain gets.
fn subject_scope_key(expr: &SubjectExpr) -> String {
    // Collect the spine outermost-first, then render it from the base out.
    let mut spine = vec![expr];
    while let Some(base) = spine
        .last()
        .expect("spine is seeded with `expr`")
        .scope_key_base()
    {
        spine.push(base);
    }

    let mut key = spine
        .last()
        .expect("spine is seeded with `expr`")
        .to_subject_text();
    for node in spine.iter().rev().skip(1) {
        match node {
            SubjectExpr::PropertyChain { property, .. } => {
                key.push_str("->");
                key.push_str(property);
            }
            SubjectExpr::ArrayAccess { segments, .. } => {
                for seg in segments {
                    match seg {
                        BracketSegment::StringKey(s) => key.push_str(&format!("[\"{}\"]", s)),
                        BracketSegment::IntKey(n) => key.push_str(&format!("[\"{}\"]", n)),
                        // A computed index keeps its written form, matching
                        // the `$types[$i]` / `$types[$count-2]` shape
                        // `expr_to_subject_key` writes, so a guard on one
                        // read narrows the next.
                        BracketSegment::ComputedIndex(index) => {
                            key.push_str(&format!("[{}]", index))
                        }
                        BracketSegment::ElementAccess => key.push_str("[]"),
                    }
                }
            }
            SubjectExpr::CallExpr { callee, .. } => {
                let SubjectExpr::MethodCall { method, .. } = callee.as_ref() else {
                    unreachable!("only a method call link is descended into")
                };
                key.push_str("->");
                key.push_str(method);
                key.push_str("()");
            }
            _ => unreachable!("the spine only descends through the links rendered here"),
        }
    }
    key
}

/// Consult the forward-walker scope for a narrowed type for a compound
/// subject key (property path like `$a->b->c`, an argument-less call like
/// `$a->b()`, or array access like `$a["k"]`).
///
/// The forward walker seeds and narrows these keys while walking the
/// enclosing body, so its scope at the subject's position is what every
/// check and write on the way there left the key holding.
///
/// Returns `Some(types)` only when the scope holds a non-empty type for
/// `key`; the caller then trusts it.  Returns `None` when the key was
/// never seeded, so the caller falls back to the declared type.
fn lookup_scope_for_subject(key: &str, ctx: &ResolutionCtx<'_>) -> Option<Vec<ResolvedType>> {
    use crate::type_engine::variable::forward_walk;

    // During diagnostic passes the forward walker records scope
    // snapshots for the whole method; these are the authority.  Skip
    // while the snapshots are still being built (the walker is the
    // authority then and re-entry would be incomplete).
    // A snapshot exists but this key was never seeded → fall through to
    // normal resolution rather than short-circuiting to empty.
    if forward_walk::is_diagnostic_scope_active()
        && !forward_walk::is_building_scopes()
        && let Some(types) = forward_walk::lookup_diagnostic_scope(key, ctx.cursor_offset)
        && !types.is_empty()
    {
        return Some(types);
    }

    // A resolution made inside a forward walk carries the walk's live
    // scope, which is the authority there.
    if let Some(resolver) = ctx.scope_var_resolver {
        let types = resolver(key);
        return (!types.is_empty()).then_some(types);
    }

    // A request made outside any walk (hover, completion, go-to-definition)
    // asks the walker what the key holds at the cursor, exactly as a
    // variable is resolved: the walk threads property and call keys
    // through every check and write on the way there.
    if !forward_walk::is_diagnostic_scope_active() {
        let dummy_class;
        let current_class = match ctx.current_class {
            Some(cc) => cc,
            None => {
                dummy_class =
                    crate::class_lookup::class_context_placeholder(ctx.content, ctx.cursor_offset);
                &dummy_class
            }
        };
        let types = super::variable::resolution::resolve_variable_types(
            key,
            current_class,
            ctx.all_classes,
            ctx.content,
            ctx.cursor_offset,
            ctx.class_loader,
            ctx.backend,
            Loaders::with_function(ctx.function_loader),
        );
        if !types.is_empty() {
            return Some(types);
        }
    }

    None
}
