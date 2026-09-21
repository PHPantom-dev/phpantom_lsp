use super::*;
use std::collections::HashSet;
use std::sync::Arc;

use crate::atom::{Atom, AtomMap, AtomSet, atom};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

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
    pub assertions: &'a AtomMap<Vec<VarAssertion>>,
    pub non_null_implications: &'a AtomMap<Vec<Atom>>,
    pub implied_narrowings: &'a AtomMap<Vec<ImpliedNarrowing>>,
    pub preg_outcomes: &'a AtomMap<PregOutcome>,
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
    pub locals: AtomMap<Vec<ResolvedType>>,

    /// Boolean variable name → the checks its value stands for.
    ///
    /// PHPStan calls these conditional expressions: the boolean carries
    /// the assertion from the expression it was assigned, so testing it
    /// narrows the original subject.
    pub assertions: AtomMap<Vec<VarAssertion>>,

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
    pub non_null_implications: AtomMap<Vec<Atom>>,

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
    pub implied_narrowings: AtomMap<Vec<ImpliedNarrowing>>,

    /// Variable name → the `preg_match` outcome its value is.
    ///
    /// The same idea as `assertions`, for the one check whose subject is
    /// an out-parameter rather than the tested expression itself.
    pub preg_outcomes: AtomMap<PregOutcome>,

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
    pub unresolved: AtomSet,

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
    pub ruled_out: AtomMap<Vec<PhpType>>,

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
            locals: AtomMap::default(),
            assertions: AtomMap::default(),
            non_null_implications: AtomMap::default(),
            implied_narrowings: AtomMap::default(),
            preg_outcomes: AtomMap::default(),
            unresolved: AtomSet::default(),
            ruled_out: AtomMap::default(),
            unreachable: false,
        }
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
        self.locals.entry(atom(var_name)).or_default();
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
        self.unresolved.insert(key);
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
        self.invalidate_proofs(var_name);
    }

    /// Remove synthetic keys that read `var_name` — a path rooted at it
    /// (`$s->cache`, `$s["k"]`) or a call that takes it as an argument
    /// (`findPos($s, $marker)`).  Called when the variable is reassigned:
    /// the value the key was recorded against is gone, so whatever was
    /// tracked for it describes the old one.
    pub fn invalidate_dependent_keys(&mut self, var_name: &str) {
        self.locals.retain(|key, _| {
            !crate::type_engine::types::narrowing::key_reads_variable(key, var_name)
        });
    }

    /// Drop what an impure call on `receiver` could have changed: every
    /// recorded call read through it, and every check whose subject is
    /// one.
    ///
    /// The receiver keeps its own type — a call does not replace the
    /// object the variable holds — and so does a property path
    /// (`$stmt->row`) or an element (`$stmt["id"]`) read through it.
    /// What goes is the recorded call (`$stmt->fetch('id')`), which is
    /// the case that matters: proving `$stmt->fetch('id') !== false`
    /// says nothing about what the same call returns once
    /// `$stmt->execute()` has run.
    ///
    /// Dropping the property paths as well would be sound — the callee
    /// may write to any of them — but it costs far more than it buys.
    /// Guard, call, use (`if (!$p->id) { throw; } $o = $p->load(); f($p->id);`)
    /// is ordinary code, and forgetting the guard there reports a null
    /// the program has already ruled out. PHPStan keeps property fetches
    /// across a method call for the same reason, and Psalm keeps them
    /// across anything it can see is pure.
    ///
    /// `made` is the key of the call doing the invalidating, when it has
    /// one, and is kept. A proof about `$s->getClassReflection()` is a
    /// proof about what that call returns, so evaluating it is the thing
    /// the proof is about rather than an event that invalidates it —
    /// dropping it would make the guard-then-use idiom hold for exactly
    /// one use, which is not what a `@phpstan-assert` tag promises.
    pub fn invalidate_receiver_state(&mut self, receiver: &str, made: Option<&str>) {
        let reads_receiver = |key: &str| {
            key != receiver
                && Some(key) != made
                && crate::type_engine::types::narrowing::is_call_key(key)
                && crate::type_engine::types::narrowing::key_reads_variable(key, receiver)
        };
        self.locals.retain(|key, _| !reads_receiver(key));
        self.non_null_implications
            .retain(|_, implied| !implied.iter().any(|k| reads_receiver(k)));
        self.implied_narrowings
            .retain(|_, narrowed| !narrowed.iter().any(|proof| reads_receiver(&proof.key)));
        if self.assertions.is_empty() {
            return;
        }
        self.assertions.retain(|_, checks| {
            checks.retain(|c| !reads_receiver(&c.subject));
            !checks.is_empty()
        });
    }

    /// Drop the proofs that writing to `var_name` invalidates: whatever
    /// the variable itself stood for, plus every proof whose subject
    /// reads it.  A boolean only describes the value its subject held
    /// when the check ran, a `?->` chain only describes the receiver it
    /// was evaluated against, and two variables a branch filled together
    /// stop being a pair the moment one of them is written on its own.
    pub fn invalidate_proofs(&mut self, var_name: &str) {
        let key = atom(var_name);
        let stale = |subject: &Atom| {
            *subject == key
                || crate::type_engine::types::narrowing::key_reads_variable(subject, var_name)
        };
        if !self.assertions.is_empty() {
            self.assertions.remove(&key);
            self.assertions.retain(|_, checks| {
                checks.retain(|c| !stale(&c.subject));
                !checks.is_empty()
            });
        }
        if !self.non_null_implications.is_empty() {
            self.non_null_implications.remove(&key);
            self.non_null_implications
                .retain(|holder, implied| !stale(holder) && !implied.iter().any(stale));
        }
        if !self.implied_narrowings.is_empty() {
            self.implied_narrowings.remove(&key);
            self.implied_narrowings.retain(|holder, narrowed| {
                !stale(holder) && !narrowed.iter().any(|proof| stale(&proof.key))
            });
        }
        if !self.preg_outcomes.is_empty() {
            self.preg_outcomes.remove(&key);
            self.preg_outcomes
                .retain(|holder, outcome| !stale(holder) && !stale(&outcome.matches_var));
        }
        if !self.ruled_out.is_empty() {
            self.ruled_out.retain(|subject, _| !stale(subject));
        }
    }

    /// Record that a check on this path ruled `excluded` out for
    /// `var_name`.
    ///
    /// See [`Self::ruled_out`] for why the exclusion has to be written
    /// down rather than left to the narrowed type to carry.
    pub fn record_exclusion(&mut self, var_name: &str, excluded: &PhpType) {
        let entry = self.ruled_out.entry(atom(var_name)).or_default();
        if !entry.contains(excluded) {
            entry.push(excluded.clone());
        }
    }

    /// Record that proving `holder` non-null proves each of `implied`
    /// non-null too.
    pub fn record_non_null_implication(&mut self, holder: &str, implied: Vec<Atom>) {
        if implied.is_empty() {
            return;
        }
        self.non_null_implications.insert(atom(holder), implied);
    }

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
        !types.is_empty() && !types.iter().any(|rt| type_admits_null(&rt.type_string))
    })
}

/// Whether `null` is one of the values this type spans.
/// Whether `ty` can hold `null`: `?T`, a union with a nullable member,
/// `null` itself, or `mixed`.
pub(crate) fn type_admits_null(ty: &PhpType) -> bool {
    match ty.kind() {
        crate::php_type::TypeKind::Nullable(_) => true,
        crate::php_type::TypeKind::Union(members) => members.iter().any(type_admits_null),
        _ => ty.is_null() || ty.is_mixed(),
    }
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
