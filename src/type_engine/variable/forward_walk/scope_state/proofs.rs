//! The proofs a scope carries beside its types, and how two paths' proofs
//! join: exclusions, non-null implications and implied narrowings.

use std::collections::HashMap;
use std::ops::ControlFlow;

use super::merge::same_types;
use super::*;
use crate::php_type::TypeKind;
use crate::type_engine::variable::forward_walk::is_synthetic_key;

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
///
/// A union is compared member by member: `1|2` and `2|3` are neither a
/// subtype of the other, yet both hold `2`.
pub(crate) fn types_are_disjoint(a: &[ResolvedType], b: &[ResolvedType]) -> bool {
    /// An array type, as opposed to `iterable`, which objects satisfy too.
    fn is_array_value(ty: &PhpType) -> bool {
        let name = match ty.kind() {
            TypeKind::Named(name) => name,
            TypeKind::Generic(generic) => &generic.name,
            _ => return ty.is_array_like(),
        };
        ty.is_array_like() && !name.eq_ignore_ascii_case("iterable")
    }

    /// Subtyping alone would call `non-empty-array<int, 1|2>` and
    /// `non-empty-array<int, 2|3>` disjoint, though both hold `[2]`, and
    /// would compare every pair of members of two element unions to say
    /// so. Two arrays that can both be empty share `[]`. Otherwise a value
    /// they share holds an entry, and that entry's value is one both
    /// element types describe, so they are disjoint when those are.
    /// Shapes keep the subtyping rule, which is what tells two tagged
    /// shapes (`array{kind: 'a'}`, `array{kind: 'b'}`) apart.
    fn arrays_disjoint(x: &PhpType, y: &PhpType) -> bool {
        if !x.is_provably_non_empty() && !y.is_provably_non_empty() {
            return false;
        }
        if x.is_empty_array_shape() || y.is_empty_array_shape() {
            return true;
        }
        let is_shape =
            |ty: &PhpType| matches!(ty.kind(), TypeKind::ArrayShape(_) | TypeKind::ListShape(_));
        if is_shape(x) && is_shape(y) {
            return !x.is_subtype_of(y) && !y.is_subtype_of(x);
        }
        match (x.iterable_element_type(), y.iterable_element_type()) {
            (Some(x_values), Some(y_values)) => disjoint(&x_values, &y_values),
            _ => false,
        }
    }

    fn disjoint(x: &PhpType, y: &PhpType) -> bool {
        match (x.kind(), y.kind()) {
            (TypeKind::Union(members), _) => members.iter().all(|m| disjoint(m, y)),
            (TypeKind::Nullable(inner), _) => disjoint(inner, y) && disjoint(&PhpType::null(), y),
            (_, TypeKind::Union(_) | TypeKind::Nullable(_)) => disjoint(y, x),
            _ if is_array_value(x) && is_array_value(y) => arrays_disjoint(x, y),
            _ => {
                !x.is_subtype_of(y)
                    && !y.is_subtype_of(x)
                    && !(x.is_object_like() && y.is_object_like())
            }
        }
    }

    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.iter()
        .all(|x| b.iter().all(|y| disjoint(&x.type_string, &y.type_string)))
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
pub(super) fn join_ruled_out(a: &ScopeState, b: &ScopeState) -> ProofMap<Vec<PhpType>> {
    if a.ruled_out.is_empty() || b.ruled_out.is_empty() {
        return ProofMap::default();
    }
    // An entry both paths still share is its own intersection.
    let mut joined = a.ruled_out.clone();
    let mut changed: Vec<(Atom, Vec<PhpType>)> = Vec::new();
    let _ = a.ruled_out.diff::<()>(&b.ruled_out, |key, mine, theirs| {
        let both: Vec<PhpType> = match (mine, theirs) {
            (Some(mine), Some(theirs)) => mine
                .iter()
                .filter(|ty| theirs.contains(ty))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        changed.push((*key, both));
        ControlFlow::Continue(())
    });
    for (key, both) in changed {
        if both.is_empty() {
            joined.remove(&key);
        } else {
            joined.insert(key, both);
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
pub(super) fn same_implied_narrowings(
    a: &AtomTrie<Vec<ImpliedNarrowing>>,
    b: &AtomTrie<Vec<ImpliedNarrowing>>,
) -> bool {
    a.len() == b.len()
        && a.diff(b, |_, mine, theirs| {
            let same = match (mine, theirs) {
                (Some(mine), Some(theirs)) => {
                    mine.len() == theirs.len()
                        && mine.iter().zip(theirs).all(|(p, q)| {
                            p.key == q.key
                                && same_types(&p.types, &q.types)
                                && same_trigger(&p.trigger, &q.trigger)
                        })
                }
                _ => false,
            };
            if same {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        })
        .is_continue()
}

/// The holders whose proof lists two maps do not share.
///
/// A holder outside this list carries the very same proofs on both paths,
/// and a proof both paths carry survives the join, so a joined map can
/// start out as either side's and only revisit these.
fn differing_holders<V>(a: &AtomTrie<V>, b: &AtomTrie<V>) -> Vec<Atom> {
    let mut holders = Vec::new();
    let _ = a.diff::<()>(b, |holder, _, _| {
        holders.push(*holder);
        ControlFlow::Continue(())
    });
    holders
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
///
/// `differing` lists the keys the two paths do not share an entry for (see
/// `ScopeState::differing_keys`); every other key holds the same value on
/// both, and so cannot be one they disagree about.
pub(super) fn join_non_null_implications(
    a: &ScopeState,
    b: &ScopeState,
    differing: &[Atom],
) -> ProofMap<Vec<Atom>> {
    let mut joined = a.non_null_implications.clone();
    let revisit = differing_holders(&a.non_null_implications, &b.non_null_implications);
    for holder in &revisit {
        joined.remove(holder);
    }
    let mut record = |holder: Atom, implied: Atom| {
        joined.push_unique(holder, implied, |a, b| a == b);
    };

    for holder in &revisit {
        let mine = a.non_null_implications.get(holder).into_iter().flatten();
        let theirs = b.non_null_implications.get(holder).into_iter().flatten();
        for implied in mine.chain(theirs) {
            if implication_holds(a, holder, implied) && implication_holds(b, holder, implied) {
                record(*holder, *implied);
            }
        }
    }

    // The variables the two paths disagree about the nullness of, split by
    // which path is the one that left them holding a value.
    let mut non_null_in_a: Vec<Atom> = Vec::new();
    let mut non_null_in_b: Vec<Atom> = Vec::new();
    for key in differing {
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
///
/// Only the keys in `differing` can disagree, as for
/// [`join_non_null_implications`], with one exception: a recorded
/// exclusion can rule out the very value both paths hold, so the keys
/// either side has excluded something for are looked at too.
pub(super) fn join_implied_narrowings(
    a: &ScopeState,
    b: &ScopeState,
    differing: &[Atom],
) -> ProofMap<Vec<ImpliedNarrowing>> {
    let mut joined = a.implied_narrowings.clone();
    // The holders whose lists the join rebuilds or adds to, written back
    // into `joined` once it is done.
    let mut lists: HashMap<Atom, ProofList> = HashMap::new();

    // Whether a proof the other path recorded is true of `side`, whose
    // own proofs for the holder are `recorded`.
    let survives = |side: &ScopeState,
                    recorded: &[ImpliedNarrowing],
                    index: &KeyIndex,
                    holder: &Atom,
                    proof: &ImpliedNarrowing| {
        index.any(recorded, &proof.key, |p| same_types(&p.types, &proof.types))
            || side
                .locals
                .get(&proof.key)
                .is_some_and(|t| same_types(t, &proof.types))
            // Asked last because the holder's type can be a large shape
            // and the trigger another one.
            || match &proof.trigger {
                // The proof is vacuous on a path whose holder could never
                // meet the trigger: that path cannot be the one a later
                // test showing the trigger is pointing at.
                ProofTrigger::NonNull => is_definitely_null(side, holder),
                ProofTrigger::Within(trigger) => side
                    .locals
                    .get(holder)
                    .is_some_and(|held| types_are_disjoint(held, trigger)),
                ProofTrigger::Outside(trigger) => side
                    .locals
                    .get(holder)
                    .is_some_and(|held| types_within(held, trigger)),
            }
    };
    for holder in differing_holders(&a.implied_narrowings, &b.implied_narrowings) {
        let mine = a
            .implied_narrowings
            .get(&holder)
            .map_or(&[][..], Vec::as_slice);
        let theirs = b
            .implied_narrowings
            .get(&holder)
            .map_or(&[][..], Vec::as_slice);
        let (mine_index, theirs_index) = (KeyIndex::of(mine), KeyIndex::of(theirs));
        // A proof is true of the path that recorded it, so only the other
        // one has to be asked. Neither path's list repeats itself, so the
        // only repeat to skip is one of `b`'s that asks what one of `a`'s
        // already does.
        let mut kept: Vec<ImpliedNarrowing> = mine
            .iter()
            .filter(|proof| survives(b, theirs, &theirs_index, &holder, proof))
            .cloned()
            .collect();
        let kept_index = KeyIndex::of(&kept);
        let kept_from_mine = kept.len();
        for proof in theirs {
            let repeated = kept_index.any(&kept[..kept_from_mine], &proof.key, |p| {
                same_trigger(&p.trigger, &proof.trigger)
            });
            if !repeated && survives(a, mine, &mine_index, &holder, proof) {
                kept.push(proof.clone());
            }
        }
        lists.insert(holder, ProofList::of(kept));
    }
    let mut record = |holder: Atom, proof: ImpliedNarrowing| {
        lists
            .entry(holder)
            .or_insert_with(|| ProofList::of(joined.get(&holder).cloned().unwrap_or_default()))
            .push_unique(proof);
    };

    // The keys the two paths disagree about, each with the triggers that
    // recognise the path whose value proved something.  Grouped by which
    // path that is, because reading a path's proofs off costs a walk of
    // everything it holds and one walk answers for every trigger that
    // points at it.
    let mut flipped: Vec<(Atom, bool, Vec<ProofTrigger>)> = Vec::new();
    let excluded = a
        .ruled_out
        .keys()
        .chain(b.ruled_out.keys())
        .filter(|key| !differing.contains(key));
    for key in differing.iter().chain(excluded) {
        // Triggers that identify `a` as the path that ran, and ones that
        // identify `b`.
        let (mut from_a, mut from_b) = (Vec::new(), Vec::new());
        // A holder one path leaves exactly `null` and the other leaves
        // holding a value identifies the path either way round: a value
        // proves the second ran, and `null` proves the first did.  The
        // second reading is what a guard written the other way round
        // needs: past `if ($a === null && $b === null) { return; }`, an
        // `if ($a === null)` is reached only where `$b` held a value.
        if is_definitely_null(a, key) && is_definitely_non_null(b, key) {
            from_b.push(ProofTrigger::NonNull);
            from_a.push(ProofTrigger::Within(
                a.locals.get(key).cloned().unwrap_or_default(),
            ));
        } else if is_definitely_null(b, key) && is_definitely_non_null(a, key) {
            from_a.push(ProofTrigger::NonNull);
            from_b.push(ProofTrigger::Within(
                b.locals.get(key).cloned().unwrap_or_default(),
            ));
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
        let pinned: Vec<PinnedOffsets> = triggers.iter().map(PinnedOffsets::new).collect();
        for key in differing {
            let Some(types) = taken.locals.get(key) else {
                continue;
            };
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
            for (trigger, pinned) in triggers.iter().zip(&pinned) {
                if pinned.pins(&holder, key, types) {
                    continue;
                }
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

    for (holder, list) in lists {
        if list.items.is_empty() {
            joined.remove(&holder);
        } else {
            joined.insert(holder, list.items);
        }
    }
    joined
}

/// What a `Within` trigger's shape says each of the holder's offsets
/// holds, read off the first time a proof about one of them comes up.
///
/// A proof that the holder being within the shape puts at offset `k` what
/// the shape itself puts there tells a later test nothing the trigger does
/// not, and a branch that fills an array one key at a time would otherwise
/// record one such proof per key at every join.
struct PinnedOffsets<'a> {
    entries: Option<&'a [crate::php_type::ShapeEntry]>,
    by_key: std::cell::OnceCell<HashMap<&'a str, &'a PhpType>>,
}

impl<'a> PinnedOffsets<'a> {
    fn new(trigger: &'a ProofTrigger) -> Self {
        let entries = match trigger {
            ProofTrigger::Within(shapes) => match shapes.as_slice() {
                [shape] => match shape.type_string.kind() {
                    TypeKind::ArrayShape(entries) => Some(&entries[..]),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        PinnedOffsets {
            entries,
            by_key: std::cell::OnceCell::new(),
        }
    }

    /// Whether the trigger already says that `key`, one of `holder`'s
    /// offsets, holds `types`.
    fn pins(&self, holder: &Atom, key: &Atom, types: &[ResolvedType]) -> bool {
        let (Some(entries), [only]) = (self.entries, types) else {
            return false;
        };
        let Some(offset) = key
            .as_str()
            .strip_prefix(holder.as_str())
            .and_then(|rest| rest.strip_prefix("[\""))
            .and_then(|rest| rest.strip_suffix("\"]"))
            .filter(|offset| !offset.contains("\"]"))
        else {
            return false;
        };
        self.by_key
            .get_or_init(|| {
                entries
                    .iter()
                    .filter(|entry| !entry.optional)
                    .filter_map(|entry| Some((entry.key.as_deref()?, &entry.value_type)))
                    .collect()
            })
            .get(offset)
            .is_some_and(|held| **held == only.type_string)
    }
}

/// Which items of a holder's proof list are about which key, kept once
/// the list is long enough that scanning all of it for every proof the
/// join looks up costs more than the index does.
#[derive(Default)]
struct KeyIndex(Option<HashMap<Atom, Vec<usize>>>);

impl KeyIndex {
    const FROM_LEN: usize = 16;

    fn of(items: &[ImpliedNarrowing]) -> Self {
        let mut index = KeyIndex::default();
        if items.len() >= Self::FROM_LEN {
            index.build(items);
        }
        index
    }

    fn build(&mut self, items: &[ImpliedNarrowing]) {
        let mut by_key: HashMap<Atom, Vec<usize>> = HashMap::new();
        for (at, item) in items.iter().enumerate() {
            by_key.entry(item.key).or_default().push(at);
        }
        self.0 = Some(by_key);
    }

    /// Note the item just pushed onto `items`.
    fn note_last(&mut self, items: &[ImpliedNarrowing]) {
        match &mut self.0 {
            Some(by_key) => {
                let at = items.len() - 1;
                by_key.entry(items[at].key).or_default().push(at);
            }
            None if items.len() >= Self::FROM_LEN => self.build(items),
            None => {}
        }
    }

    /// Whether `test` accepts an item of `items` about `key`.
    fn any(
        &self,
        items: &[ImpliedNarrowing],
        key: &Atom,
        test: impl Fn(&ImpliedNarrowing) -> bool,
    ) -> bool {
        match &self.0 {
            Some(by_key) => by_key
                .get(key)
                .is_some_and(|ats| ats.iter().any(|&at| test(&items[at]))),
            None => items.iter().any(|item| item.key == *key && test(item)),
        }
    }
}

/// A holder's proof list as a join builds it.
#[derive(Default)]
struct ProofList {
    items: Vec<ImpliedNarrowing>,
    index: KeyIndex,
}

impl ProofList {
    fn of(items: Vec<ImpliedNarrowing>) -> Self {
        let index = KeyIndex::of(&items);
        ProofList { items, index }
    }

    /// Add `proof` unless the list already asks the same of its holder
    /// about the same key.
    fn push_unique(&mut self, proof: ImpliedNarrowing) {
        if self.index.any(&self.items, &proof.key, |p| {
            same_trigger(&p.trigger, &proof.trigger)
        }) {
            return;
        }
        self.items.push(proof);
        self.index.note_last(&self.items);
    }
}
