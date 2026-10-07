mod class_unions;
mod locals;
mod merge;
mod proof_map;
mod proofs;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
mod trie;

pub(crate) use class_unions::*;
pub(crate) use locals::Locals;
pub(crate) use proof_map::ProofMap;
pub(crate) use proofs::*;
pub(crate) use trie::AtomTrie;

use crate::atom::{Atom, atom};
use crate::php_type::PhpType;
use crate::types::ResolvedType;

// ─── Core data structures ───────────────────────────────────────────────────

/// An `instanceof`-style check that a boolean variable stands for.
///
/// `$isHtml = $raw instanceof HtmlString;` records `subject = "$raw"`
/// under `$isHtml`, so a later truthy test on `$isHtml` narrows `$raw`
/// exactly as the original expression does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VarAssertion {
    /// Scope key the check narrows (`"$raw"`, `"$this->node"`, …).
    pub subject: Atom,
    /// The type checked against.
    pub class_type: PhpType,
    /// The further types the check allows, when the boolean was assigned
    /// an `||` chain over one subject
    /// (`$isNode = $n instanceof Stmt || $n instanceof Expr`).  The
    /// subject is one of `class_type` and these, not all of them.
    pub alternatives: Vec<PhpType>,
    /// The check was written negated (`$notHtml = !$raw instanceof …`).
    pub negated: bool,
    /// Exact class identity (`get_class($raw) === …`) rather than a
    /// subtype check.
    pub exact: bool,
    /// The check was `is_a($raw, Foo::class, true)` — a string
    /// alternative on the subject must survive narrowing.
    pub allow_string: bool,
}

/// The `preg_match` outcome a variable holds the result of.
///
/// `$ok = preg_match('/(\d+)/', $s, $m);` records `$m` and the shape a
/// successful match leaves in it under `$ok`, so a later test on `$ok`
/// narrows `$m` exactly as testing the call itself does. The shape is
/// stored rather than the call, because the condition that tests it is
/// somewhere else entirely and has no view of the pattern.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PregOutcome {
    /// Scope key the outcome narrows (`"$m"`).
    pub matches_var: Atom,
    /// The shape a successful match leaves in it.
    pub matched: PhpType,
    /// The call was `preg_match_all`, whose failed match is shaped
    /// differently from `preg_match`'s.
    pub matches_all: bool,
}

/// Which of the keys read through an object a call on it invalidates.
///
/// See [`ScopeState::invalidate_receiver_state`].
pub(crate) enum MemberInvalidation {
    /// Only the recorded results of calls on it.
    Calls,
    /// Every key read through it, except the ones listed.
    Members { kept: Vec<String> },
}

/// One thing invoking a closure literal does to a capture, worked out once
/// when the closure is assigned to a variable rather than re-derived from
/// its body at every call site.
///
/// `$cb = function () { $this->stop(); }; call_user_func($cb);` invalidates
/// `$this` exactly as if `$this->stop()` were written inline at the
/// `call_user_func` call, but by the time that call is walked the closure's
/// body is gone from view — only the variable that named it is left. See
/// [`ScopeState::closure_capture_effects`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ClosureCaptureEffect {
    /// Scope key the effect targets (e.g. `"$this"`, `"$counter"`).
    pub subject: Atom,
    /// Key of the call doing the invalidating, whose own proof survives.
    pub made: Option<Atom>,
    /// Whether property paths through the subject go too, not just call
    /// results.
    pub members: bool,
    /// The method called on the subject, when it is the receiver.
    pub method: Option<Atom>,
}

/// What has to be shown about a proof's holder before the proof applies.
#[derive(Clone, Debug)]
pub(crate) enum ProofTrigger {
    /// The holder's `null` is gone.
    ///
    /// The idiom below a join usually spells this out
    /// (`if ($original !== null)`), and it is the one trigger that needs
    /// no types of its own.
    NonNull,
    /// The holder is one of these — the value the path that recorded the
    /// proof left it as.
    ///
    /// Every branch condition other than a null check proves a *type*:
    /// `count($args) > 0` proves `$args` is a `non-empty-array`, so
    /// re-testing that condition below the join is recognised by the type
    /// it re-establishes.
    Within(Vec<ResolvedType>),
    /// The holder is none of these — the value the *other* path left it
    /// as.
    ///
    /// The complement of [`Self::Within`], and the only reading available
    /// when the path that proved something left the holder exactly as it
    /// found it: a value that contradicts what the other path left is
    /// proof that path did not run, and the two paths of a join are
    /// exhaustive, so this one did.  `if (!$isI && !$isJ) { return; }` is
    /// the shape that needs it — past the guard, `$isI` being `false` is
    /// the only thing that says `$isJ` held, and the path that proved
    /// `$isJ` never tested `$isI` at all.
    Outside(Vec<ResolvedType>),
}

/// What a branch proved about one value, to be re-applied wherever its
/// holder is shown to have taken that branch.
#[derive(Clone, Debug)]
pub(crate) struct ImpliedNarrowing {
    /// What the holder has to be shown to be before the proof applies.
    pub trigger: ProofTrigger,
    /// The key the proof is about.
    pub key: Atom,
    /// The types it held on the path that proved it.
    pub types: Vec<ResolvedType>,
}

/// The proofs a scope holds about values other than their own types,
/// borrowed from the scope that recorded them.
///
/// A condition is narrowed against a `ScopeState` built for the occasion
/// in [`condition_arm_narrowing`](super::condition_arm_narrowing), which
/// only knows the types of the subjects the condition names.  A boolean
/// standing for a check, a `preg_match` result, and a pair of variables
/// filled together are all proofs the condition never names outright, so
/// they have to travel with the resolution context to reach it.
#[derive(Clone, Copy)]
pub(crate) struct ScopeProofs<'a> {
    pub assertions: &'a ProofMap<Vec<VarAssertion>>,
    pub non_null_implications: &'a ProofMap<Vec<Atom>>,
    pub implied_narrowings: &'a ProofMap<Vec<ImpliedNarrowing>>,
    pub preg_outcomes: &'a ProofMap<PregOutcome>,
}

impl ScopeProofs<'_> {
    /// Whether nothing is recorded at all, which is the common case and
    /// lets callers skip the seeding work entirely.
    pub fn is_empty(&self) -> bool {
        self.assertions.is_empty()
            && self.non_null_implications.is_empty()
            && self.implied_narrowings.is_empty()
            && self.preg_outcomes.is_empty()
    }

    /// The keys the proofs recorded under `holder` are about.
    ///
    /// Reading a proof back needs the subject's own type in scope: an
    /// `instanceof` recorded under `$isFoo` narrows `$node`, so a
    /// condition that only names `$isFoo` still has to have `$node`
    /// seeded before the narrowing has anything to act on.
    pub fn subjects_of(&self, holder: &Atom, out: &mut Vec<String>) {
        let mut push = |key: &Atom| {
            let key = key.to_string();
            if !out.contains(&key) {
                out.push(key);
            }
        };
        if let Some(checks) = self.assertions.get(holder) {
            for check in checks {
                push(&check.subject);
            }
        }
        if let Some(implied) = self.non_null_implications.get(holder) {
            for key in implied {
                push(key);
            }
        }
        if let Some(narrowed) = self.implied_narrowings.get(holder) {
            for proof in narrowed {
                push(&proof.key);
            }
        }
        if let Some(outcome) = self.preg_outcomes.get(holder) {
            push(&outcome.matches_var);
        }
    }
}

/// Which keys arrays are known to hold on one path, for writing through
/// a key PHP already has.
///
/// A write through such a key lands on an entry that is already there, so
/// it keeps the array's key type, its list promise, and whatever it said
/// about being empty. PHPStan calls this a write to an existing offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct KeyFacts {
    /// (array subject key, key variable) pairs: the key variable holds a
    /// key the array has, because a `foreach` over the array bound it and
    /// nothing since has removed an entry.
    pub existing_keys: Vec<(Atom, Atom)>,
    /// Pairs of subject keys known to hold the same keys: the value
    /// variable a keyed `foreach` binds and the element it was read from
    /// (`$inner` and `$convert[$outerKey]`). A key a nested `foreach` over
    /// one of them binds is then an existing key of the other as well.
    pub key_set_aliases: Vec<(Atom, Atom)>,
}

/// The type-state of all variables at a single program point.
///
/// This is the equivalent of PHPStan's `expressionTypes` map and Mago's
/// `BlockContext.locals`.  It is created once at the start of a function
/// body analysis, seeded with parameter types, and passed as `&mut` through
/// the forward walk.
#[derive(Clone, Debug)]
pub(crate) struct ScopeState {
    /// Variable name (with `$` prefix, e.g. `"$foo"`) → resolved types.
    ///
    /// This is the single source of truth for all variable types at the
    /// current program point.  Every variable that has been assigned,
    /// declared as a parameter, or bound by a foreach/catch before the
    /// current statement has an entry here.
    pub locals: Locals,

    /// Boolean variable name → the checks its value stands for.
    ///
    /// PHPStan calls these conditional expressions: the boolean carries
    /// the assertion from the expression it was assigned, so testing it
    /// narrows the original subject.
    pub assertions: ProofMap<Vec<VarAssertion>>,

    /// Scope key → the keys that proving it non-null also proves non-null.
    ///
    /// Two sources feed this.  A `?->` chain records its receivers under
    /// the key it was stored in: `$period = $agreement?->latestPeriod();`
    /// records `$agreement` under `$period`, because the chain yields
    /// `null` for a null receiver, so a guard that later rules out
    /// `$period`'s null rules out `$agreement`'s with it.  A branch join
    /// records the variables it saw flip from `null` to a value together
    /// (see [`ScopeState::merge_branch`]), which is what lets a later
    /// check on one of them recover what it implies about the others.
    ///
    /// Either way the proof is one the guard's own condition never names.
    pub non_null_implications: ProofMap<Vec<Atom>>,

    /// Scope key → the types other keys held on the path that left it
    /// holding a value.
    ///
    /// What [`Self::non_null_implications`] is to `null`, for every other
    /// narrowing a branch made.  A branch that narrows a subject and
    /// fills a variable in the same step makes the variable stand for the
    /// narrowing, so a test on the variable anywhere below is a test of
    /// whether the branch ran:
    ///
    /// ```php
    /// $original = null;
    /// if ($stmt->valueVar instanceof Variable) {
    ///     $original = new OriginalValue($stmt->valueVar->name);
    /// }
    /// // … 60 lines on …
    /// if ($original !== null) { $stmt->valueVar->name; }  // still a Variable
    /// ```
    pub implied_narrowings: ProofMap<Vec<ImpliedNarrowing>>,

    /// Variable name → the `preg_match` outcome its value is.
    ///
    /// The same idea as `assertions`, for the one check whose subject is
    /// an out-parameter rather than the tested expression itself.
    pub preg_outcomes: ProofMap<PregOutcome>,

    /// The names whose empty [`Self::locals`] entry stands for a value
    /// the engine failed to work out, rather than one that could be
    /// anything.
    ///
    /// The two are the same thing to a reader — nothing is known either
    /// way — but they are opposites at a join.  A value that could be
    /// anything is the top of the lattice and swallows what the other
    /// path proved; a value we simply could not compute is a gap in our
    /// own analysis, reported where it happened, and has no business
    /// erasing anything.  See [`ScopeState::merge_branch`].
    pub unresolved: AtomTrie<()>,

    /// Scope key → the classes this path has shown the value is *not*.
    ///
    /// The failing side of an `instanceof` is the one narrowing PHP's type
    /// language cannot spell: the else of `if ($id instanceof B)` on an
    /// `A` leaves `A` behind, because "an `A` that is not a `B`" has no
    /// notation.  So the two paths of that `if` describe values that
    /// cannot both be the one in hand, while the types they leave say they
    /// overlap.  Recording the class the check ruled out is what tells
    /// [`Self::merge_branch`] otherwise, and so lets a later re-test of
    /// the same check recover what the branch it guarded wrote.
    ///
    /// Only the join reads this, so it does not travel with
    /// [`ScopeProofs`]: below the join the exclusion holds only where both
    /// incoming paths made it, which is exactly what the join leaves
    /// behind.
    pub ruled_out: ProofMap<Vec<PhpType>>,

    /// Variable name → what invoking the closure literal it was last
    /// assigned does to its captures, precomputed at the assignment so a
    /// later call through the variable can apply it without the closure's
    /// body in view.
    ///
    /// Cleared whenever the variable is reassigned or removed (see
    /// [`Self::invalidate_proofs`]), the same as every other proof keyed on
    /// a variable's identity.
    pub closure_captures: AtomTrie<Vec<ClosureCaptureEffect>>,

    /// What this path knows about which keys arrays hold; see
    /// [`KeyFacts`]. Boxed and `None` while there is nothing to say, which
    /// is almost always: scopes are cloned and cached by the thousand.
    pub key_facts: Option<Box<KeyFacts>>,

    /// No value can reach this program point.
    ///
    /// Set when a condition narrows some variable down to nothing — the
    /// implicit else of `if ($v instanceof AbstractNode)` where `$v` is
    /// already an `AbstractNode`, for instance.  Such a path contributes
    /// nothing to a join: the types it carries describe a run of the
    /// program that cannot happen, and merging them widens the result
    /// back to the pre-branch type the branch was supposed to replace.
    pub unreachable: bool,
}

impl ScopeState {
    /// A variable resolver that answers from a snapshot of this scope.
    ///
    /// Taking a copy is what lets the resolver be handed to the RHS
    /// pipeline while the walker still holds the scope mutably:
    /// resolution cannot see, or be disturbed by, writes made after the
    /// snapshot was taken.
    pub(crate) fn snapshot_resolver(&self) -> impl Fn(&str) -> Vec<ResolvedType> + use<> {
        let locals = self.locals.clone();
        move |var_name: &str| locals.get(&atom(var_name)).cloned().unwrap_or_default()
    }

    pub fn new() -> Self {
        Self {
            locals: Locals::default(),
            assertions: ProofMap::default(),
            non_null_implications: ProofMap::default(),
            implied_narrowings: ProofMap::default(),
            preg_outcomes: ProofMap::default(),
            unresolved: AtomTrie::default(),
            ruled_out: ProofMap::default(),
            closure_captures: AtomTrie::default(),
            key_facts: None,
            unreachable: false,
        }
    }

    /// What invoking the closure literal `var_name` last held does to its
    /// captures, or `&[]` when it never held one (or the closure's body has
    /// no state-changing effect worth recording).
    pub fn closure_capture_effects(&self, var_name: &str) -> &[ClosureCaptureEffect] {
        self.closure_captures
            .get(&atom(var_name))
            .map_or(&[], |v| v.as_slice())
    }

    /// Record what invoking the closure literal just assigned to `var_name`
    /// does to its captures.  A no-op for an empty list, matching
    /// [`Self::set`]: nothing worth recording is the same as nothing
    /// recorded.
    pub fn set_closure_capture_effects(
        &mut self,
        var_name: &str,
        effects: Vec<ClosureCaptureEffect>,
    ) {
        if effects.is_empty() {
            return;
        }
        self.closure_captures.insert(atom(var_name), effects);
    }

    /// Borrow the proofs this scope holds that are not variable types.
    pub fn proofs(&self) -> ScopeProofs<'_> {
        ScopeProofs {
            assertions: &self.assertions,
            non_null_implications: &self.non_null_implications,
            implied_narrowings: &self.implied_narrowings,
            preg_outcomes: &self.preg_outcomes,
        }
    }

    /// Copy another scope's proofs into this one.
    pub fn adopt_proofs(&mut self, proofs: &ScopeProofs<'_>) {
        self.assertions = proofs.assertions.clone();
        self.non_null_implications = proofs.non_null_implications.clone();
        self.implied_narrowings = proofs.implied_narrowings.clone();
        self.preg_outcomes = proofs.preg_outcomes.clone();
    }

    /// Look up a variable's types.  Returns an empty slice when the
    /// variable has not been assigned.
    pub fn get(&self, var_name: &str) -> &[ResolvedType] {
        self.locals
            .get(&atom(var_name))
            .map_or(&[], |v| v.as_slice())
    }

    /// Check whether a variable exists in scope (even if its type list is empty).
    pub fn contains(&self, var_name: &str) -> bool {
        self.locals.contains_key(&atom(var_name))
    }

    /// Insert or overwrite a variable's types.  An empty type list is
    /// ignored, leaving any existing entry untouched; use
    /// [`Self::set_empty`] to record existence without types.
    pub fn set(&mut self, var_name: &str, types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        let key = atom(var_name);
        self.unresolved.remove(&key);
        self.locals.insert(key, types);
    }

    /// Record that a variable exists in scope with an empty type list,
    /// so passes that iterate the scope's keys (e.g. condition
    /// narrowing) can see it even though no type is known yet.
    pub fn set_empty(&mut self, var_name: &str) {
        self.locals.get_or_insert_default(atom(var_name));
    }

    /// Replace whatever was known about a variable with "no type known".
    ///
    /// Unlike [`Self::set_empty`], this overwrites an existing entry: it
    /// is what an assignment whose right-hand side resolves to nothing
    /// records, since the old value is gone whether or not the new one
    /// could be typed.
    ///
    /// The entry is flagged in [`Self::unresolved`], which is what keeps
    /// it from erasing the other paths' types at the next join.
    pub fn set_unknown(&mut self, var_name: &str) {
        let key = atom(var_name);
        self.locals.insert(key, Vec::new());
        self.unresolved.insert(key, ());
    }

    /// Replace whatever was known about a variable with "could be
    /// anything".
    ///
    /// The counterpart to [`Self::set_unknown`], for a narrowing that
    /// landed on a class the loader cannot supply
    /// (`assert($n instanceof SomeUnindexedClass)`).  The constraint the
    /// program states is real and it does bound the value; we just have
    /// nothing in scope that spells it out.  That is the top of the
    /// lattice, so it absorbs at a join instead of standing aside the
    /// way an unresolved entry does.
    pub fn set_untyped(&mut self, var_name: &str) {
        let key = atom(var_name);
        self.locals.insert(key, Vec::new());
        self.unresolved.remove(&key);
    }

    /// Insert a variable's types from parameter seeding.
    pub fn seed(&mut self, var_name: &str, types: Vec<ResolvedType>) {
        if types.is_empty() {
            return;
        }
        let key = atom(var_name);
        self.unresolved.remove(&key);
        self.locals.insert(key, types);
    }

    /// Remove a variable (e.g. after `unset($x)`).
    pub fn remove(&mut self, var_name: &str) {
        let key = atom(var_name);
        self.locals.remove(&key);
        self.unresolved.remove(&key);
        self.closure_captures.remove(&key);
        self.invalidate_proofs(var_name);
    }

    /// Remove synthetic keys that read `var_name` — a path rooted at it
    /// (`$s->cache`, `$s["k"]`) or a call that takes it as an argument
    /// (`findPos($s, $marker)`).  Called when the variable is reassigned:
    /// the value the key was recorded against is gone, so whatever was
    /// tracked for it describes the old one.
    pub fn invalidate_dependent_keys(&mut self, var_name: &str) {
        self.locals.retain_compound(|key| {
            !crate::type_engine::types::narrowing::key_reads_variable(key, var_name)
        });
    }

    /// Drop what a call on `receiver` could have changed.
    ///
    /// The receiver keeps its own type — a call does not replace the
    /// object the variable holds.  What goes depends on `members`:
    ///
    /// - [`MemberInvalidation::Calls`] drops every recorded call read
    ///   through it (`$stmt->fetch('id')`), and every check whose subject
    ///   is one.  Proving `$stmt->fetch('id') !== false` says nothing about
    ///   what the same call returns once `$stmt->execute()` has run.  A
    ///   property path (`$stmt->row`) or an element (`$stmt["id"]`) read
    ///   through it stays: this is what a call we cannot classify, or a
    ///   write to one of the object's properties, costs.
    /// - [`MemberInvalidation::Members`] also drops the property paths and
    ///   elements, except the ones listed as kept (a readonly property
    ///   cannot have been written).  This is for a call known to change
    ///   state: one that returns nothing, returns `$this`, or is declared
    ///   impure.  A call that computes a value keeps them all, which is
    ///   what keeps guard, call, use (`if (!$p->id) { throw; } $o =
    ///   $p->load(); f($p->id);`) working; PHPStan draws the line in the
    ///   same place.
    ///
    /// `made` is the key of the call doing the invalidating, when it has
    /// one, and is kept. A proof about `$s->getClassReflection()` is a
    /// proof about what that call returns, so evaluating it is the thing
    /// the proof is about rather than an event that invalidates it —
    /// dropping it would make the guard-then-use idiom hold for exactly
    /// one use, which is not what a `@phpstan-assert` tag promises.
    pub fn invalidate_receiver_state(
        &mut self,
        receiver: &str,
        made: Option<&str>,
        members: &MemberInvalidation,
    ) {
        let reads_receiver = |key: &str| {
            key != receiver
                && Some(key) != made
                && crate::type_engine::types::narrowing::key_reads_variable(key, receiver)
                && match members {
                    MemberInvalidation::Calls => {
                        crate::type_engine::types::narrowing::is_call_key(key)
                    }
                    MemberInvalidation::Members { kept } => !kept.iter().any(|k| k == key),
                }
        };
        self.locals.retain_compound(|key| !reads_receiver(key));
        self.non_null_implications
            .retain_mentioning(receiver, |_, implied| {
                !implied.iter().any(|k| reads_receiver(k))
            });
        self.implied_narrowings
            .retain_mentioning(receiver, |_, narrowed| {
                !narrowed.iter().any(|proof| reads_receiver(&proof.key))
            });
        self.assertions
            .retain_items_mentioning(receiver, |c| !reads_receiver(&c.subject));
    }

    /// Drop the proofs that writing to `var_name` invalidates: whatever
    /// the variable itself stood for, plus every proof whose subject
    /// reads it.  A boolean only describes the value its subject held
    /// when the check ran, a `?->` chain only describes the receiver it
    /// was evaluated against, and two variables a branch filled together
    /// stop being a pair the moment one of them is written on its own.
    pub fn invalidate_proofs(&mut self, var_name: &str) {
        let key = atom(var_name);
        self.closure_captures.remove(&key);
        let stale = |subject: &Atom| {
            *subject == key
                || crate::type_engine::types::narrowing::key_reads_variable(subject, var_name)
        };
        if !self.assertions.is_empty() {
            self.assertions.remove(&key);
            self.assertions
                .retain_items_mentioning(var_name, |c| !stale(&c.subject));
        }
        if !self.non_null_implications.is_empty() {
            self.non_null_implications.remove(&key);
            self.non_null_implications
                .retain_mentioning(var_name, |holder, implied| {
                    !stale(holder) && !implied.iter().any(stale)
                });
        }
        if !self.implied_narrowings.is_empty() {
            self.implied_narrowings.remove(&key);
            self.implied_narrowings
                .retain_mentioning(var_name, |holder, narrowed| {
                    !stale(holder) && !narrowed.iter().any(|proof| stale(&proof.key))
                });
        }
        if !self.preg_outcomes.is_empty() {
            self.preg_outcomes.remove(&key);
            self.preg_outcomes
                .retain_mentioning(var_name, |holder, outcome| {
                    !stale(holder) && !stale(&outcome.matches_var)
                });
        }
        if !self.ruled_out.is_empty() {
            self.ruled_out
                .retain_mentioning(var_name, |subject, _| !stale(subject));
        }
        self.retain_key_facts(
            |subject, key_var| !stale(subject) && *key_var != key,
            |side| !stale(side),
        );
    }

    /// Whether `key_var` holds a key the array `subject` is known to have.
    pub fn is_existing_key(&self, subject: &str, key_var: &str) -> bool {
        self.key_facts.as_ref().is_some_and(|facts| {
            facts
                .existing_keys
                .iter()
                .any(|(s, k)| s.as_str() == subject && k.as_str() == key_var)
        })
    }

    /// Record that a `foreach` over `subject` just bound `key_var` to one of
    /// its keys, and `value_var` (when there is one) to the entry there.
    ///
    /// Whatever the two variables stood for on the previous iteration is
    /// dropped first. A key of `subject` is also a key of every subject
    /// known to hold the same keys, and the entry there holds the same keys
    /// as the value variable does.
    pub fn record_foreach_keys(&mut self, subject: &str, key_var: &str, value_var: Option<&str>) {
        let key = atom(key_var);
        let names_a_bound_var = |side: &Atom| {
            let reads = |var: &str| {
                side.as_str() == var
                    || crate::type_engine::types::narrowing::key_reads_variable(side, var)
            };
            reads(key_var) || value_var.is_some_and(reads)
        };
        self.retain_key_facts(
            |s, k| *k != key && !names_a_bound_var(s),
            |side| !names_a_bound_var(side),
        );
        let facts = self.key_facts.get_or_insert_with(Box::default);
        let subject_atom = atom(subject);
        let mut subjects = vec![subject_atom];
        for (a, b) in &facts.key_set_aliases {
            if *a == subject_atom {
                subjects.push(*b);
            } else if *b == subject_atom {
                subjects.push(*a);
            }
        }
        for s in &subjects {
            facts.existing_keys.push((*s, key));
        }
        if let Some(value_var) = value_var {
            let value = atom(value_var);
            for s in subjects {
                facts
                    .key_set_aliases
                    .push((value, atom(&format!("{s}[{key_var}]"))));
            }
        }
    }

    /// Drop what an element write or removal on the array `subject` may
    /// have invalidated.
    ///
    /// `keys_lost` marks a write that replaced the value at `subject`, or an
    /// `unset()` that removed one of its entries: every key fact about
    /// `subject` and what is read through it goes. Otherwise the write only
    /// added a key to it, which keeps the keys every subject is known to
    /// have but ends any claim that another subject holds the same ones.
    pub fn note_element_write(&mut self, subject: &str, keys_lost: bool) {
        if self.key_facts.is_none() {
            return;
        }
        let at_or_below = |side: &Atom| {
            side.as_str() == subject
                || crate::type_engine::types::narrowing::key_reads_variable(side, subject)
        };
        if keys_lost {
            self.retain_key_facts(|s, _| !at_or_below(s), |side| !at_or_below(side));
        } else {
            self.retain_key_facts(|_, _| true, |side| side.as_str() != subject);
        }
    }

    /// Keep the existing-key facts `keep_key` accepts and the aliases whose
    /// both sides `keep_side` accepts, going back to `None` once nothing is
    /// left.
    fn retain_key_facts(
        &mut self,
        keep_key: impl Fn(&Atom, &Atom) -> bool,
        keep_side: impl Fn(&Atom) -> bool,
    ) {
        let Some(facts) = self.key_facts.as_mut() else {
            return;
        };
        facts.existing_keys.retain(|(s, k)| keep_key(s, k));
        facts
            .key_set_aliases
            .retain(|(a, b)| keep_side(a) && keep_side(b));
        if facts.existing_keys.is_empty() && facts.key_set_aliases.is_empty() {
            self.key_facts = None;
        }
    }

    /// Record that a check on this path ruled `excluded` out for
    /// `var_name`.
    ///
    /// See [`Self::ruled_out`] for why the exclusion has to be written
    /// down rather than left to the narrowed type to carry.
    pub fn record_exclusion(&mut self, var_name: &str, excluded: &PhpType) {
        self.ruled_out
            .push_unique(atom(var_name), excluded.clone(), |a, b| a == b);
    }

    /// Record that proving `holder` non-null proves each of `implied`
    /// non-null too.
    pub fn record_non_null_implication(&mut self, holder: &str, implied: Vec<Atom>) {
        if implied.is_empty() {
            return;
        }
        self.non_null_implications.insert(atom(holder), implied);
    }
}
