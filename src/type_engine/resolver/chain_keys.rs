//! The keys a subject chain is looked up under before anything is
//! resolved: each link's chain cache key, and the forward walker's scope
//! key for the links it can narrow.

use crate::type_engine::subject_expr::{BracketSegment, SubjectExpr};
use crate::types::*;

use super::ResolutionCtx;
use super::context::{CHAIN_CACHE, ChainKey, ChainSite, NameScope};

/// The cached result of a chain link, when the chain cache holds one.
pub(super) fn lookup_chain_cache(cache_key: &ChainKey) -> Option<Vec<ResolvedType>> {
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
pub(super) struct SpineKeys {
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
    pub(super) fn new(spine: &[(&SubjectExpr, AccessKind)], ctx: &ResolutionCtx<'_>) -> Self {
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

        // Within one file the same text still means different things where
        // its names are read differently: `self::make()` in two classes,
        // or `make()` in two `namespace` blocks that import different
        // functions.  An entry shared across positions is shared only
        // where the names read the same.  Worked out on the first link
        // that is cached at all.
        let mut names = None;

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
                names: *names.get_or_insert_with(|| NameScope::of(ctx)),
                chain: chains[index],
                site,
            });
        }
        keys
    }

    pub(super) fn cache_key(&self, index: usize) -> Option<&ChainKey> {
        self.cache_keys[index].as_ref()
    }

    /// The forward-walker scope key of link `index`, or `None` when the
    /// walker never narrows that link.
    pub(super) fn scope_key(&self, index: usize) -> Option<&str> {
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
pub(super) fn subject_scope_key(expr: &SubjectExpr) -> String {
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
pub(super) fn lookup_scope_for_subject(
    key: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
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
        let types = ctx.resolve_variable_at_cursor(key);
        if !types.is_empty() {
            return Some(types);
        }
    }

    None
}
