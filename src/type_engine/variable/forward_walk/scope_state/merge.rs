use std::collections::HashSet;
use std::sync::Arc;

use super::*;
use crate::type_engine::variable::forward_walk::is_synthetic_key;
use crate::types::ClassInfo;

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

        for (name, other_types) in &other.locals {
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
                continue;
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
                continue;
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

        self.non_null_implications = implications;
        self.implied_narrowings = narrowings;
        self.ruled_out = exclusions;
    }
}

/// Whether two type lists say the same thing, comparing a shared
/// `class_info` by pointer rather than walking the class.
fn same_types(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
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

/// Whether no single value could be described by both type lists.
///
/// Deliberately conservative: two lists count as disjoint only when every
/// pairing of their members is, and a pair only counts when neither member
/// is a subtype of the other and they are not both objects (a class the
/// loader would have to be consulted about is left overlapping rather than
/// guessed at). Answering "disjoint" wrongly would let a later test
/// re-apply a proof from a branch that never ran — `bool` and `true` are
/// the shape that matters, since a branch a plain boolean guards leaves
/// the flag `bool` on the path that skipped it.
pub(crate) fn types_are_disjoint(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.iter().all(|x| {
        b.iter().all(|y| {
            let (x, y) = (&x.type_string, &y.type_string);
            !x.is_subtype_of(y)
                && !y.is_subtype_of(x)
                && !(x.is_object_like() && y.is_object_like())
        })
    })
}

/// Whether `side` has shown that `key` cannot be holding any of `types`.
///
/// What a failed `instanceof` proves is not in the type it leaves behind —
/// an `A` that is not a `B` is still spelled `A` — so a value the other
/// path narrowed to `B` reads as one this path could be holding too. The
/// recorded exclusion is what says otherwise.
///
/// The two spellings are compared by class name as well as by subtyping:
/// the exclusion is written down as the condition spelled it, while the
/// other path's type carries the name the class loader resolved it to.
fn path_rules_out(side: &ScopeState, key: &Atom, types: &[ResolvedType]) -> bool {
    if types.is_empty() {
        return false;
    }
    let Some(excluded) = side.ruled_out.get(key) else {
        return false;
    };
    let same_class = |held: &PhpType, gone: &PhpType| match (
        held.unwrap_nullable().class_name(),
        gone.unwrap_nullable().class_name(),
    ) {
        (Some(h), Some(g)) => h
            .trim_start_matches('\\')
            .eq_ignore_ascii_case(g.trim_start_matches('\\')),
        _ => false,
    };
    types.iter().all(|held| {
        excluded
            .iter()
            .any(|gone| held.type_string.is_subtype_of(gone) || same_class(&held.type_string, gone))
    })
}

/// The exclusions that survive a join: a value is only known not to be a
/// class when neither incoming path could have left it as one.
fn join_ruled_out(a: &ScopeState, b: &ScopeState) -> AtomMap<Vec<PhpType>> {
    if a.ruled_out.is_empty() || b.ruled_out.is_empty() {
        return AtomMap::default();
    }
    let mut joined: AtomMap<Vec<PhpType>> = AtomMap::default();
    for (key, mine) in &a.ruled_out {
        let Some(theirs) = b.ruled_out.get(key) else {
            continue;
        };
        let both: Vec<PhpType> = mine
            .iter()
            .filter(|ty| theirs.contains(ty))
            .cloned()
            .collect();
        if !both.is_empty() {
            joined.insert(*key, both);
        }
    }
    joined
}

/// Whether two proofs ask the same thing of their holder.
fn same_trigger(a: &ProofTrigger, b: &ProofTrigger) -> bool {
    match (a, b) {
        (ProofTrigger::NonNull, ProofTrigger::NonNull) => true,
        (ProofTrigger::Within(x), ProofTrigger::Within(y))
        | (ProofTrigger::Outside(x), ProofTrigger::Outside(y)) => same_types(x, y),
        _ => false,
    }
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

/// Whether every value `narrow` describes is one `wide` describes too.
fn types_within(narrow: &[ResolvedType], wide: &[ResolvedType]) -> bool {
    !narrow.is_empty()
        && !wide.is_empty()
        && narrow.iter().all(|n| {
            wide.iter()
                .any(|w| n.type_string.is_subtype_of(&w.type_string))
        })
}

/// Whether two implied-narrowing maps record the same proofs.
fn same_implied_narrowings(
    a: &AtomMap<Vec<ImpliedNarrowing>>,
    b: &AtomMap<Vec<ImpliedNarrowing>>,
) -> bool {
    a.len() == b.len()
        && a.iter().all(|(holder, mine)| {
            b.get(holder).is_some_and(|theirs| {
                mine.len() == theirs.len()
                    && mine.iter().zip(theirs).all(|(p, q)| {
                        p.key == q.key
                            && same_types(&p.types, &q.types)
                            && same_trigger(&p.trigger, &q.trigger)
                    })
            })
        })
}

/// Whether a scope's entry for `key` holds exactly `null` and nothing else.
fn is_definitely_null(scope: &ScopeState, key: &Atom) -> bool {
    scope
        .locals
        .get(key)
        .is_some_and(|types| !types.is_empty() && types.iter().all(|rt| rt.type_string.is_null()))
}

/// Whether a scope's entry for `key` rules `null` out.
///
/// An absent or untyped entry does not: an unknown value could be
/// anything, `null` included.
fn is_definitely_non_null(scope: &ScopeState, key: &Atom) -> bool {
    scope.locals.get(key).is_some_and(|types| {
        !types.is_empty() && !types.iter().any(|rt| rt.type_string.accepts_null())
    })
}

/// Whether "`holder` non-null proves `implied` non-null" is true of the
/// values a single path carries.
///
/// Three ways it can be: the path recorded the proof itself, the path
/// leaves `holder` null so the claim is vacuous there, or the path leaves
/// `implied` non-null so the claim holds whatever `holder` is.
fn implication_holds(scope: &ScopeState, holder: &Atom, implied: &Atom) -> bool {
    scope
        .non_null_implications
        .get(holder)
        .is_some_and(|implieds| implieds.contains(implied))
        || is_definitely_null(scope, holder)
        || is_definitely_non_null(scope, implied)
}

/// The non-null proofs that hold at the join of two paths.
///
/// A proof either path carries survives when it is true of both — the
/// vacuity above is what lets a `?->` chain proof recorded inside a branch
/// outlive the join with a path that never ran the assignment and left the
/// holder null.
///
/// The join also *learns* proofs the two paths never wrote down. Variables
/// that one path leaves null and the other leaves non-null were written
/// together, so each one's null stands for the other's:
///
/// ```php
/// $acceptor = null;
/// $reflection = null;
/// if ($name !== '') {
///     $reflection = $this->find($name);
///     if ($reflection !== null) {
///         $acceptor = $this->select($reflection);
///     }
/// }
/// // Either both are null or neither is, so a later `$reflection !== null`
/// // rules out `$acceptor`'s null as well.
/// ```
///
/// Only a variable each path pins down either way takes part. One that is
/// nullable on either side was not written by the branch as a whole (a
/// loop that assigns it under its own condition, say), and says nothing
/// about what the other variables did.
fn join_non_null_implications(a: &ScopeState, b: &ScopeState) -> AtomMap<Vec<Atom>> {
    let mut joined: AtomMap<Vec<Atom>> = AtomMap::default();
    let mut record = |holder: Atom, implied: Atom| {
        let entry: &mut Vec<Atom> = joined.entry(holder).or_default();
        if !entry.contains(&implied) {
            entry.push(implied);
        }
    };

    for (holder, implieds) in a
        .non_null_implications
        .iter()
        .chain(b.non_null_implications.iter())
    {
        for implied in implieds {
            if implication_holds(a, holder, implied) && implication_holds(b, holder, implied) {
                record(*holder, *implied);
            }
        }
    }

    // The variables the two paths disagree about the nullness of, split by
    // which path is the one that left them holding a value.
    let mut non_null_in_a: Vec<Atom> = Vec::new();
    let mut non_null_in_b: Vec<Atom> = Vec::new();
    for key in a.locals.keys() {
        if is_definitely_null(a, key) {
            if is_definitely_non_null(b, key) {
                non_null_in_b.push(*key);
            }
        } else if is_definitely_null(b, key) && is_definitely_non_null(a, key) {
            non_null_in_a.push(*key);
        }
    }

    for group in [&non_null_in_a, &non_null_in_b] {
        for holder in group.iter() {
            for implied in group.iter().filter(|k| *k != holder) {
                record(*holder, *implied);
            }
        }
    }

    joined
}

/// The narrowings that hold at the join of two paths, conditional on a
/// key holding a value.
///
/// A proof either path recorded survives when it is true of both: the
/// other path recorded it too, leaves the holder null so the claim is
/// vacuous there, or already agrees about the key's type.
///
/// The join also *learns* proofs from a key the two paths disagree about,
/// whenever the disagreement is one a later test can settle. That key was
/// written or narrowed by the path that narrowed everything else the paths
/// disagree about, so re-establishing its value below the join is testing
/// which path ran:
///
/// ```php
/// $original = null;
/// if ($stmt->valueVar instanceof Variable) { $original = new Value(); }
/// if ($original !== null) { /* $stmt->valueVar is a Variable here */ }
/// ```
///
/// Nullness is one such disagreement, and the one a plain `!== null` test
/// spells out. Any other pair of *disjoint* types settles the question
/// just as well, and is what makes re-testing a condition re-apply what it
/// proved the first time:
///
/// ```php
/// if (count($args) > 0) { $acceptor = Selector::selectFromArgs(…); }
/// if (count($args) > 0) { /* $acceptor is not null here */ }
/// ```
///
/// Disjointness is what makes either sound. The two paths have to describe
/// values that cannot both be the one in hand, or a later test that
/// matches the taken path's type would also have matched the skipped
/// path's and would prove nothing.
///
/// Which keys take part depends on how they are spelled: a plain
/// variable has to be typed on both paths, since one a path never bound
/// is a branch-local assignment rather than a narrowing of a value that
/// existed before the branch. A property path or a call key is readable
/// on both paths whatever either recorded for it, so the path that
/// narrowed it contributes even when the other left no entry at all.
fn join_implied_narrowings(a: &ScopeState, b: &ScopeState) -> AtomMap<Vec<ImpliedNarrowing>> {
    let mut joined: AtomMap<Vec<ImpliedNarrowing>> = AtomMap::default();
    let mut record = |holder: Atom, proof: ImpliedNarrowing| {
        let entry: &mut Vec<ImpliedNarrowing> = joined.entry(holder).or_default();
        let already = entry
            .iter()
            .any(|p| p.key == proof.key && same_trigger(&p.trigger, &proof.trigger));
        if !already {
            entry.push(proof);
        }
    };

    let survives = |side: &ScopeState, holder: &Atom, proof: &ImpliedNarrowing| {
        side.implied_narrowings.get(holder).is_some_and(|proofs| {
            proofs
                .iter()
                .any(|p| p.key == proof.key && same_types(&p.types, &proof.types))
        }) || match &proof.trigger {
            // The proof is vacuous on a path whose holder could never meet
            // the trigger: that path cannot be the one a later test
            // showing the trigger is pointing at.
            ProofTrigger::NonNull => is_definitely_null(side, holder),
            ProofTrigger::Within(trigger) => side
                .locals
                .get(holder)
                .is_some_and(|held| types_are_disjoint(held, trigger)),
            ProofTrigger::Outside(trigger) => side
                .locals
                .get(holder)
                .is_some_and(|held| types_within(held, trigger)),
        } || side
            .locals
            .get(&proof.key)
            .is_some_and(|t| same_types(t, &proof.types))
    };
    for (holder, proofs) in a
        .implied_narrowings
        .iter()
        .chain(b.implied_narrowings.iter())
    {
        for proof in proofs {
            if survives(a, holder, proof) && survives(b, holder, proof) {
                record(*holder, proof.clone());
            }
        }
    }

    // The keys the two paths disagree about, each with the triggers that
    // recognise the path whose value proved something.  Grouped by which
    // path that is, because reading a path's proofs off costs a walk of
    // everything it holds and one walk answers for every trigger that
    // points at it.
    let mut flipped: Vec<(Atom, bool, Vec<ProofTrigger>)> = Vec::new();
    for key in a.locals.keys() {
        // Triggers that identify `a` as the path that ran, and ones that
        // identify `b`.
        let (mut from_a, mut from_b) = (Vec::new(), Vec::new());
        if is_definitely_null(a, key) && is_definitely_non_null(b, key) {
            from_b.push(ProofTrigger::NonNull);
        } else if is_definitely_null(b, key) && is_definitely_non_null(a, key) {
            from_a.push(ProofTrigger::NonNull);
        } else if let (Some(mine), Some(theirs)) = (a.locals.get(key), b.locals.get(key)) {
            if types_are_disjoint(mine, theirs) {
                // Either path's value settles which one ran, so both are
                // worth recording: the `if` arm's type re-proves what the
                // arm wrote, and the `else` arm's re-proves what that one
                // did.
                from_a.push(ProofTrigger::Within(mine.clone()));
                from_b.push(ProofTrigger::Within(theirs.clone()));
            } else {
                // Types that overlap on paper but not in fact, because
                // the check one path failed took its proof with it.
                // Recognising a path by its own value only works in the
                // direction the exclusion covers: the path that ruled the
                // other's value out cannot be holding it, while its own
                // value is one the excluding path's type still spans.
                if path_rules_out(b, key, mine) {
                    from_a.push(ProofTrigger::Within(mine.clone()));
                }
                if path_rules_out(a, key, theirs) {
                    from_b.push(ProofTrigger::Within(theirs.clone()));
                }
                // Whichever way round, a value that contradicts what one
                // path left is proof that path did not run — and the two
                // paths are exhaustive, so the other one did.  This is the
                // only reading available when the path that proved
                // something left the holder exactly as it found it, which
                // is what an `||` guard's fall-through does to the flag
                // its *other* leg tested.
                //
                // Only where one path's value sits strictly inside the
                // other's, which is the disagreement this reading is for:
                // one path narrowed the holder and the other left it
                // spanning what that narrowing picked out.  Between values
                // that merely fail to contain one another the trigger
                // still holds, but it is met by any later narrowing of the
                // holder at all, whether or not the guard had anything to
                // do with it — a proof per differing key at every join, to
                // re-apply what a path did not touch.
                if types_within(mine, theirs) && !types_within(theirs, mine) {
                    from_b.push(ProofTrigger::Outside(mine.clone()));
                }
                if types_within(theirs, mine) && !types_within(mine, theirs) {
                    from_a.push(ProofTrigger::Outside(theirs.clone()));
                }
            }
        }
        if !from_a.is_empty() {
            flipped.push((*key, true, from_a));
        }
        if !from_b.is_empty() {
            flipped.push((*key, false, from_b));
        }
    }
    for (holder, taken_is_a, triggers) in flipped {
        let (taken, skipped) = if taken_is_a { (a, b) } else { (b, a) };
        for (key, types) in &taken.locals {
            if *key == holder || types.is_empty() {
                continue;
            }
            let differs = match skipped.locals.get(key) {
                Some(other) => !same_types(types, other),
                // A path that never bound a plain variable did not
                // narrow it, it never had it: what the other path left
                // there is an assignment rather than a proof about a
                // value that existed before the branch.  A property path
                // or a call key is readable on both paths, so an absent
                // entry only means this one recorded no narrowing for it.
                None => is_synthetic_key(key.as_str()),
            };
            if !differs {
                continue;
            }
            for trigger in &triggers {
                record(
                    holder,
                    ImpliedNarrowing {
                        trigger: trigger.clone(),
                        key: *key,
                        types: types.clone(),
                    },
                );
            }
        }
    }

    joined
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
pub(crate) fn drop_branch_local_virtual_members(
    existing: &mut ResolvedType,
    incoming: &ResolvedType,
) {
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

/// Simplify unions in a scope by collapsing child/parent class pairs.
///
/// When merging branches produces a union like `Child | Parent` where
/// `Child extends Parent`, the union is redundant — every value of
/// type `Child` is also a `Parent`.  This collapses such unions to
/// the broadest (parent) type.
///
/// Entries that do not name a class (scalars, array shapes, generics
/// whose base is not class-like) are left alone, as are the entries of a
/// variable with only one alternative.  A `?Child` alternative counts as
/// naming its inner class, and its nullability is carried over to the
/// parent that subsumes it — dropping `?Child` in favour of a
/// non-nullable `Parent` would silently lose the null.
pub(crate) fn simplify_class_hierarchy_unions(
    scope: &mut ScopeState,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) {
    let keys: Vec<Atom> = scope.locals.keys().copied().collect();
    for key in keys {
        // Decide what to drop under an immutable borrow so the class
        // names can be borrowed rather than cloned, then apply the
        // decision under a mutable one.
        let Some(types) = scope.locals.get(&key) else {
            continue;
        };
        if types.len() < 2 {
            continue;
        }

        // (index, class name, admits null) for every alternative that
        // names a class.
        let named: Vec<(usize, &str, bool)> = types
            .iter()
            .enumerate()
            .filter_map(|(idx, rt)| {
                rt.type_string
                    .unwrap_nullable()
                    .class_name()
                    .map(|name| (idx, name, rt.type_string.accepts_null()))
            })
            .collect();
        if named.len() < 2 {
            continue;
        }

        let mut dropped = vec![false; types.len()];
        let mut widen_to_nullable = vec![false; types.len()];
        for &(parent_idx, parent_name, parent_nullable) in &named {
            if dropped[parent_idx] {
                continue;
            }
            for &(child_idx, child_name, child_nullable) in &named {
                if child_idx == parent_idx || dropped[child_idx] {
                    continue;
                }
                if is_subclass_of(child_name, parent_name, class_loader) {
                    dropped[child_idx] = true;
                    if child_nullable && !parent_nullable {
                        widen_to_nullable[parent_idx] = true;
                    }
                }
            }
        }
        if !dropped.iter().any(|d| *d) {
            continue;
        }

        let Some(types) = scope.locals.get_mut(&key) else {
            continue;
        };
        for (idx, widen) in widen_to_nullable.iter().enumerate() {
            if *widen && !dropped[idx] {
                let widened = types[idx].type_string.clone().or_null();
                types[idx].type_string = widened;
            }
        }
        let mut idx = 0;
        types.retain(|_| {
            let keep = !dropped[idx];
            idx += 1;
            keep
        });
    }
}

/// Check whether `child` is a subclass (direct or transitive) of
/// `parent`, including implemented interfaces.
///
/// Returns `false` if `child` cannot be loaded or if there is no
/// inheritance relationship.  Delegates to the shared nominal subtype
/// walk ([`crate::class_lookup::is_subtype_of`]), which handles
/// transitive interface extension, FQN normalisation, and cycle
/// detection.
pub(crate) fn is_subclass_of(
    child: &str,
    parent: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    if child.eq_ignore_ascii_case(parent) {
        return false; // same class, not a subclass
    }
    match class_loader(child) {
        Some(child_class) => crate::class_lookup::is_subtype_of(&child_class, parent, class_loader),
        None => false,
    }
}
