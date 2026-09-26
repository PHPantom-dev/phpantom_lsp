use std::collections::HashSet;
use std::sync::Arc;

use super::proofs::{
    join_implied_narrowings, join_non_null_implications, join_ruled_out, same_implied_narrowings,
};
use super::*;

impl ScopeState {
    /// Whether two scopes say the same thing about every name they hold.
    ///
    /// A cheap stand-in for a full structural comparison: the two sides of
    /// the check that matters are clones of one another, so a shared
    /// `class_info` compares by pointer and never walks a class.
    fn describes_same_state_as(&self, other: &ScopeState) -> bool {
        if self.locals.len() != other.locals.len()
            || self.unresolved != other.unresolved
            || self.assertions != other.assertions
            || self.non_null_implications != other.non_null_implications
            || self.preg_outcomes != other.preg_outcomes
            || self.closure_captures != other.closure_captures
            || !same_implied_narrowings(&self.implied_narrowings, &other.implied_narrowings)
        {
            return false;
        }
        self.locals
            .iter()
            .all(|(name, types)| other.locals.get(name).is_some_and(|t| same_types(types, t)))
    }

    /// Merge another scope into `self`.
    ///
    /// For each variable:
    /// - Present in both, both typed: union the type sets (variable was
    ///   assigned in both branches).
    /// - Present in both, either side untyped: untyped, because an entry
    ///   with no types stands for a value that exists and could be
    ///   anything.  Unknown is the *top* of the type lattice, not the
    ///   bottom, so joining it with a type yields unknown again — this is
    ///   what stops a branch-local proof about an untyped subject
    ///   (`if ($version instanceof Foo)` on a `stdClass` property) from
    ///   escaping the join.
    /// - Present in both, one side [`unresolved`](Self::unresolved): the
    ///   other side's types, because that path did not *observe* a value
    ///   that could be anything, it failed to work one out.  The failure
    ///   is reported where it happened, and the join has no more reason
    ///   to spread it than an unreachable path has to contribute types.
    /// - Present in only one: keep it with the existing types (variable
    ///   was assigned in only one branch — it *might* have those types).
    ///
    /// After merging, subsumed entries are removed.  When one entry's
    /// type is a subset of another (e.g. `string|null` ⊆
    /// `int|string|null`, or `Foo` ⊆ `mixed`), the subset entry is
    /// dropped because the superset already covers it.  Without this,
    /// narrowed types from non-exiting if-branches leak into the
    /// post-merge scope and pollute subsequent narrowing operations.
    ///
    /// An unreachable scope is the identity of the join: it describes a
    /// run that cannot happen, so it neither contributes types nor
    /// swallows the other side's.
    pub fn merge_branch(&mut self, other: &ScopeState) {
        if other.unreachable {
            return;
        }
        if self.unreachable {
            self.clone_from(other);
            return;
        }
        // Two paths that agree on everything join to what they already
        // say.  This is the common shape for a loop or `switch` exit
        // edge — the trailing `break;` of an arm leaves with exactly the
        // state the arm ends with — and skipping the union keeps a
        // token-dispatch `switch` with fifty arms from re-unioning the
        // whole scope once per arm.
        if self.describes_same_state_as(other) {
            // The exclusions are still joined: two paths can leave the
            // same types behind while only one of them ruled a class out,
            // and keeping that one's word for it would let a later join
            // read a proof off a check the other path never made.  They
            // are deliberately not part of `describes_same_state_as`,
            // because re-running the union over identical locals is not a
            // no-op — `mixed` absorbs its siblings there — and a scope
            // that says the same thing must come out saying it.
            self.ruled_out = join_ruled_out(self, other);
            return;
        }

        // A boolean only still stands for a check if every incoming path
        // agrees on it.  A check one branch established (or reassigned
        // out from under) says nothing about the joined program point.
        if !self.assertions.is_empty() {
            self.assertions
                .retain(|name, checks| other.assertions.get(name) == Some(checks));
        }

        // Which non-null proofs the join keeps, and which ones it learns
        // from the two paths disagreeing.  Computed before the locals are
        // unioned below, because both answers read the per-path types.
        let implications = join_non_null_implications(self, other);
        let narrowings = join_implied_narrowings(self, other);
        let exclusions = join_ruled_out(self, other);

        // Likewise for a stored match outcome: a path that never ran the
        // call, or reassigned either half of it, leaves the boolean
        // standing for nothing at the joined point.
        if !self.preg_outcomes.is_empty() {
            self.preg_outcomes
                .retain(|name, outcome| other.preg_outcomes.get(name) == Some(outcome));
        }

        // Likewise: a variable only counts as still naming the closure it
        // was assigned when every incoming path assigned it the same one.
        if !self.closure_captures.is_empty() {
            self.closure_captures
                .retain(|name, effects| other.closure_captures.get(name) == Some(effects));
        }

        for (name, other_types) in &other.locals {
            self.merge_local(name, other_types, other);
        }

        self.non_null_implications = implications;
        self.implied_narrowings = narrowings;
        self.ruled_out = exclusions;
    }

    /// Join `other`'s entry for one local into `self`, as one step of
    /// [`merge_branch`](Self::merge_branch).
    fn merge_local(&mut self, name: &Atom, other_types: &[ResolvedType], other: &ScopeState) {
        // A path that failed to resolve the value says nothing about
        // it, so it leaves what this side carries alone.  Only a name
        // this side has never seen picks the failure up, so that a
        // later join still knows the entry stands for a gap rather
        // than for a value that could be anything.
        if other_types.is_empty() && other.unresolved.contains(name) {
            if !self.locals.contains_key(name) {
                self.locals.insert(*name, Vec::new());
                self.unresolved.insert(*name);
            }
            return;
        }

        // The same, the other way round: whatever this side lost, the
        // other path's answer stands for.  An `other_types` that is
        // empty here is a value that could be anything, which is the
        // top of the lattice and so the answer either way.
        let self_lost =
            self.unresolved.contains(name) && self.locals.get(name).is_some_and(Vec::is_empty);
        if self_lost {
            self.unresolved.remove(name);
            self.locals.insert(*name, Vec::new());
        } else if let Some(existing) = self.locals.get(name)
            && (existing.is_empty() || other_types.is_empty())
        {
            // An entry both paths carry but at least one of them has
            // no type for is unknown at the join.  Only a name the
            // other path never bound at all is adopted wholesale:
            // that is a branch-local assignment, which the walker
            // reports as a possible type rather than dropping.
            self.locals.insert(*name, Vec::new());
            return;
        }

        // Whether the two paths already say the same thing about this
        // key, which decides how far the subsumption pass below may
        // go.
        let agreed = self
            .locals
            .get(name)
            .is_some_and(|existing| same_type_strings(existing, other_types));

        let entry = self.locals.entry(*name).or_default();

        // Merge other_types into entry.  When an incoming entry
        // shares a class name with an existing entry but has a
        // broader type_string (e.g. `?A` vs `A`), widen the
        // existing entry's type_string instead of discarding
        // the incoming one.  This prevents post-loop merges from
        // losing nullable information.
        for rt in other_types.iter() {
            let mut merged_into_existing = false;
            // Set when an existing entry names the same class but
            // neither spelling covers the other, so the incoming
            // type has to be kept beside it rather than folded in.
            let mut keep_beside_same_class = false;
            if let Some(ref rt_cls) = rt.class_info {
                for existing in entry.iter_mut() {
                    if let Some(ref ex_cls) = existing.class_info
                        && ex_cls.name == rt_cls.name
                    {
                        // Same class.  If the incoming type is
                        // broader, adopt it.  If neither spelling
                        // covers the other (`?A` against the `A&B`
                        // an `instanceof` proved on the other path),
                        // there is nothing to fold into: keep
                        // looking, and let the incoming type be
                        // added as its own alternative below rather
                        // than be swallowed by whichever path the
                        // join happened to start from.
                        if existing.type_string != rt.type_string {
                            if existing.type_string.is_subset_of(&rt.type_string) {
                                existing.type_string = rt.type_string.clone();
                            } else if !rt.type_string.is_subset_of(&existing.type_string) {
                                keep_beside_same_class = true;
                                continue;
                            }
                        }
                        // A virtual member that only one branch's
                        // class_info carries (e.g. a member injected by
                        // `property_exists` / `method_exists` narrowing
                        // inside a guarded branch) must not survive the
                        // merge: the member is only proven where the
                        // guard held.  Drop any virtual member missing
                        // from the incoming branch.
                        drop_branch_local_virtual_members(existing, rt);
                        // A factory is only known to build one model
                        // (or a collection) at the join when every
                        // incoming path built the same thing.
                        existing.factory_count = existing.factory_count.join(rt.factory_count);
                        merged_into_existing = true;
                        break;
                    }
                }
            } else if rt.type_string.is_array_shape() {
                // Fold an incoming array-shape variant into an
                // existing array-shape entry instead of accumulating
                // one variant per branch (`array{a: int}` merged with
                // `array{a: int, b: string}` becomes
                // `array{a: int, b?: string}`).  A variable written
                // key-by-key across hundreds of conditionals would
                // otherwise collect hundreds of near-identical shape
                // variants, and the pairwise subsumption pass below
                // makes every subsequent merge quadratic in that
                // variant count.
                for existing in entry.iter_mut() {
                    if existing.class_info.is_none()
                        && let Some(joined) = existing.type_string.join_shapes(&rt.type_string)
                    {
                        existing.type_string = joined;
                        merged_into_existing = true;
                        break;
                    }
                }
            }
            if merged_into_existing {
                continue;
            }
            if keep_beside_same_class {
                // `push_unique` keys on the class name alone, so it
                // would drop this as a duplicate of the entry the
                // fold above just declined.  The subsumption pass
                // below is what decides which spelling survives.
                entry.push(rt.clone());
            } else {
                ResolvedType::push_unique(entry, rt.clone());
            }
        }

        // Scalar literals are exact within each branch, but a broader
        // sibling branch already covers them after control-flow rejoins.
        // Preserve class-backed alternatives (and their completion
        // metadata) while collapsing only redundant non-class values.
        *entry = ResolvedType::collapse_redundant_runtime_literals(std::mem::take(entry));

        // Remove entries whose type is subsumed by a broader entry
        // (e.g. `string|null` ⊆ `int|string|null`). `mixed_absorbs_siblings:
        // true` — unlike a ternary's arms, a non-exiting `if`'s
        // narrowing must not survive past the merge: `if ($mixed
        // instanceof Foo) { … }` with no `else` must leave plain
        // `mixed` behind, not `Foo|mixed`.
        //
        // That is a decision about what the *join* brought together,
        // so it is off for a key both paths already agreed about.
        // There the `mixed` and the class beside it are one value the
        // scope has been carrying all along — `$a = f() ?? $arg;` on a
        // `mixed`-returning `f()` — and absorbing the class would turn
        // a receiver every member lookup resolves through into one
        // that resolves to nothing, without any branch having narrowed
        // anything.
        ResolvedType::drop_subsumed_entries(entry, !agreed);
    }
}

/// Whether two type lists say the same thing, comparing a shared
/// `class_info` by pointer rather than walking the class.
pub(super) fn same_types(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.type_string == y.type_string
                && match (&x.class_info, &y.class_info) {
                    (Some(p), Some(q)) => Arc::ptr_eq(p, q),
                    (None, None) => true,
                    _ => false,
                }
        })
}

/// Whether two type lists spell out the same alternatives, in order.
///
/// Weaker than [`same_types`], which also requires a shared `class_info`
/// allocation.  Two paths can describe a value identically while having
/// rebuilt its class along the way, and for deciding whether a join
/// brought anything new together the spelling is what matters.
fn same_type_strings(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.type_string == y.type_string)
}

/// Drop virtual members from `existing`'s class_info that the `incoming`
/// branch's same-class class_info does not carry.
///
/// Branch-local narrowing (notably `property_exists` / `method_exists`)
/// injects a virtual member into a *clone* of the variable's class_info
/// for the guarded branch only.  When that branch merges with a sibling
/// that never proved the member, the union no longer guarantees it, so
/// the injected member must not leak into the merged scope.
///
/// Only virtual members are reconciled — real declared members are
/// identical across branches (same class source) and never removed.  A
/// virtual member present in *both* branches (e.g. an `@property` tag or
/// a Laravel model column baked into the base class_info) is kept,
/// because both branches derive from the same pre-branch class_info, so
/// any base virtual member appears on both sides and only narrowing-added
/// members appear on one.
fn drop_branch_local_virtual_members(existing: &mut ResolvedType, incoming: &ResolvedType) {
    let (Some(ex_cls), Some(in_cls)) = (&existing.class_info, &incoming.class_info) else {
        return;
    };
    // Same Arc → identical member sets, nothing to reconcile.  This is
    // the common case (no branch narrowed the type), so the merge stays
    // cheap.
    if Arc::ptr_eq(ex_cls, in_cls) {
        return;
    }

    let incoming_virtual_props: HashSet<&str> = in_cls
        .properties
        .iter()
        .filter(|p| p.is_virtual)
        .map(|p| p.name.as_str())
        .collect();
    let incoming_virtual_methods: HashSet<String> = in_cls
        .methods
        .iter()
        .filter(|m| m.is_virtual)
        .map(|m| m.name.to_ascii_lowercase())
        .collect();

    let drop_prop = ex_cls
        .properties
        .iter()
        .any(|p| p.is_virtual && !incoming_virtual_props.contains(p.name.as_str()));
    let drop_method = ex_cls
        .methods
        .iter()
        .any(|m| m.is_virtual && !incoming_virtual_methods.contains(&m.name.to_ascii_lowercase()));
    if !drop_prop && !drop_method {
        return;
    }

    let mut narrowed = (**ex_cls).clone();
    if drop_prop {
        narrowed
            .properties
            .make_mut()
            .retain(|p| !p.is_virtual || incoming_virtual_props.contains(p.name.as_str()));
    }
    if drop_method {
        narrowed.methods.make_mut().retain(|m| {
            !m.is_virtual || incoming_virtual_methods.contains(&m.name.to_ascii_lowercase())
        });
    }
    existing.class_info = Some(Arc::new(narrowed));
}
