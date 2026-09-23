mod class_unions;
mod merge;
mod proofs;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) use class_unions::*;
pub(crate) use proofs::*;

use crate::atom::{Atom, AtomMap, AtomSet, atom};
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
}
