<?php

/**
 * PHP Showcase — Completion
 *
 * Completion and the type inference behind it: chaining, narrowing,
 * generics, docblock tags, arrays and shapes, closures, generators, and
 * everything else that decides what the member list after `->` or `::`
 * should contain. Open any demo() method and trigger completion inside.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

use Demo\Scaffolding;
use Demo\Scaffolding\UserProfile as Profile;
use Exception;

// ═══════════════════════════════════════════════════════════════════════════
//  DEMOS — open any demo() method and trigger completion inside
// ═══════════════════════════════════════════════════════════════════════════


// ── Auto-Import (completion) ────────────────────────────────────────────────
// Try: type `new DateT` and accept `DateTime`. The `use DateTime;` statement
// is inserted at the top of the import list above to maintain alphabetical
// order (it sorts ahead of the `Demo\Scaffolding\...` imports).
//
// The `use Exception;` import above occupies the short name "Exception".
// Try: type `throw new pq\Exception()` and accept — the auto-import inserts
// `\pq\Exception` at the usage site instead of a conflicting `use` statement.

// ── Namespace Segment Completion ────────────────────────────────────────────
// Try: erase the class name after `use Demo\` and trigger completion to see
// namespace segments (module/folder icon) alongside class names.

// ── Namespaced Function Completion ──────────────────────────────────────────
// Try: type `use function parse_file` and accept to get
// `use function ast\parse_file;`


// ── Instance Completion ─────────────────────────────────────────────────────

class InstanceCompletionDemo
{
    public function demo(): void
    {
        $zoo = new Scaffolding\Zoo();

        $zoo->aardvark();            // own method
        $zoo->baboon;                // own property
        $zoo->buffalo;               // constructor-promoted property
        $zoo->cheetah;               // readonly promoted (from base)
        $zoo->dingo();               // trait method
        $zoo->elephant('Hi');        // trait method
        $zoo->falcon();              // inherited from parent
        $zoo->gorilla;               // @property (own class)
        $zoo->hyena('x');            // @method (own class)
        $zoo->iguana;                // @property-read (interface)
        $zoo->jaguar();              // @method (interface)
        // MUST NOT appear: $keeper (protected), $ceo (private), nocturnal() (private)
    }
}


// ── Mixed Accessor Chaining ─────────────────────────────────────────────────

class MixedAccessorDemo
{
    public function demo(): void
    {
        $foobar = new Scaffolding\StaticPropHolder();
        $foobar->holder::$shared;                 // $obj->prop::$static chain

        // Inline (new Foo)->method() chaining
        (new Scaffolding\Pen())->write();                     // resolves Scaffolding\Pen then write()
    }
}

// ── Pseudo-Type Class-Name Collision ────────────────────────────────────────
// A class may be named after a PHPDoc pseudo-type such as `number` (PHP 8.4
// ships `BcMath\Number`). The class must not be shadowed by the pseudo-type.

class PseudoTypeCollisionDemo
{
    public function demo(): void
    {
        $n = new Scaffolding\Number('42');

        $n->value;          // own property resolves (not treated as int|float)
        $n->scaled(2);      // own method resolves

        // A `Scaffolding\Number` parameter type resolves to the class, so passing a
        // `Scaffolding\Number` instance is accepted (no false type-mismatch diagnostic).
        Scaffolding\scaleNumber($n);
    }
}

// ── Method & Property Chaining ──────────────────────────────────────────────

class ChainingDemo
{
    public function demo(): void
    {
        $studio = new Scaffolding\ScaffoldingChainingDemo();

        // Fluent method chains — MUST NOT appear: calibrate() (protected)
        $studio->brush->setSize('large')->setStyle('pointed')->stroke();

        // Return type chains
        $studio->brush->getCanvas()->title();

        // Variable → method chain
        $canvas = $studio->brush->getCanvas();
        $canvas->getBrush()->stroke();

        // Deep property chain
        $studio->canvas->easel->material;
        $studio->canvas->easel->height();

        // Null-safe chaining
        $maybe = Scaffolding\Brush::find(1);
        $maybe?->getCanvas()?->title();

        // Multi-line method chains
        $studio->brush->setSize('large')
            ->setStyle('pointed')
            ->stroke();

        // Variable assigned from chain
        $directBrush = $studio->brush->getCanvas()->getBrush();
        $directBrush->stroke();

        // (new Class())->method()
        $fromNew = (new Scaffolding\Canvas())->getBrush();
        $fromNew->stroke();

        // Intermediate variable from property access
        $easel = (new Scaffolding\Canvas())->easel;
        $easel->material;
    }
}


// ── Trait `return $this` Fluent Chains ──────────────────────────────────────
// A trait method whose body is `return $this;` (no declared return type)
// resolves to the class that uses the trait, so a chained call continues with
// the using class's own members — not just the trait's.

class TraitFluentChainDemo
{
    public function demo(): void
    {
        $page = new Scaffolding\TestablePage();

        // Each step resolves to Scaffolding\TestablePage (the using class), so the whole
        // chain — including Scaffolding\TestablePage's own members — stays available.
        $page->assertSee('Welcome')       // trait method, body `return $this;`
            ->assertSee('Dashboard')      // still Scaffolding\TestablePage, not the trait
            ->status;                     // using class's own property

        $page->assertSee('x')->refresh(); // using class's own method resolves
    }
}


// ── @var Docblock Override ──────────────────────────────────────────────────

class VarDocblockDemo
{
    public function demo(): void
    {
        /** @var Scaffolding\Pencil $inlineHinted */
        $inlineHinted = Scaffolding\getUnknownValue();
        $inlineHinted->sketch();

        /** @var Scaffolding\Pen */
        $hinted = Scaffolding\getUnknownValue();
        $hinted->write();                         // without variable name (PHPStorm fails this)
    }
}


// ── Return Type Resolution ──────────────────────────────────────────────────

class ReturnTypeDemo
{
    public function demo(): void
    {
        $made = Scaffolding\Pen::make();                      // static return type → Scaffolding\Pen
        $made->write();

        $marker = Scaffolding\Marker::make();                 // static on subclass → Scaffolding\Marker
        $marker->highlight();                     // resolves to Scaffolding\Marker, not Scaffolding\Pen

        $fluent = $marker->rename('Bold');         // rename returns static → Scaffolding\Marker
        $fluent->highlight();                     // chained static stays on the subclass

        // Writing the class out pins it: `static` here can only ever be
        // `Scaffolding\Marker`, so hover reads `Scaffolding\Marker` and not `static(Scaffolding\Marker)`.  A
        // forwarding call is the case that stays open — see
        // LateStaticBindingDemo below.

        $created = Scaffolding\makePen();
        $created->write();                        // function return type
        // MUST NOT appear: refill() (private)

        $absolute = \Demo\Scaffolding\makePen();  // leading-backslash (absolute) call
        $absolute->write();                       // resolves the same as Scaffolding\makePen()

        $found = Scaffolding\pickPenOrPencil();               // Scaffolding\Pen|Scaffolding\Pencil union
        $found->label();                          // available on both types
    }
}


// ── Late Static Binding ─────────────────────────────────────────────────────

class LateStaticBindingDemo extends Scaffolding\Pen
{
    public function demo(): void
    {
        // These four forward late static binding, so `static` stays open:
        // it is at least LateStaticBindingDemo, and a subclass of this class
        // would get itself.  Hover shows the bound as `static(...)`.
        $forwarded = $this->rename('Bold');
        $forwarded->write();

        $viaSelf = self::make();
        $viaStatic = static::make();
        $viaParent = parent::make();
        $viaSelf->write();
        $viaStatic->write();
        $viaParent->write();                      // still bound here, not Scaffolding\Pen

        // A first-class callable resolves to the same type as the direct
        // call it stands for.
        $callable = $this->rename(...);
        $callable('Bold')->write();

        // Naming the class closes it again, even from inside the hierarchy.
        $pinned = Scaffolding\Pen::make();                    // exactly Scaffolding\Pen
        $pinned->write();
    }

    /** The forwarding forms, handed back so the assertions can check them. */
    public function forwardedInstances(): array
    {
        return [self::make(), static::make(), parent::make()];
    }
}


// ── Type Narrowing ──────────────────────────────────────────────────────────

class TypeNarrowingDemo
{
    public function demo(): void
    {
        $specimen = Scaffolding\pickRockOrBanana();           // Scaffolding\Rock|Scaffolding\Banana
        if ($specimen instanceof Scaffolding\Rock) {
            $specimen->crush();                   // narrowed to Scaffolding\Rock
            // MUST NOT appear: peel() (Scaffolding\Banana only)
        } else {
            $specimen->peel();                    // narrowed to Scaffolding\Banana (else branch)
            // MUST NOT appear: crush() (Scaffolding\Rock only)
        }

        if (!$specimen instanceof Scaffolding\Rock) {
            $specimen->peel();                    // negated instanceof
        }

        // An instanceof check rules out the array and null alternatives
        // too, not just the other class in the union.
        $picked = Scaffolding\pickRockOrRocks(true);          // Scaffolding\Rock|array<Scaffolding\Rock>|null
        if ($picked instanceof Scaffolding\Rock) {
            Scaffolding\crushOneRock($picked);                // narrowed to Scaffolding\Rock
        }

        $unknown = Scaffolding\getUnknownValue();
        if (is_a($unknown, Scaffolding\Rock::class)) {
            $unknown->crush();                    // is_a() narrowing
        }

        $target = Scaffolding\getUnknownValue();
        assert($target instanceof Scaffolding\Banana);
        $target->peel();                          // assert() narrowing

        // Inline && narrowing — RHS of && sees the narrowed type from LHS
        $sample = Scaffolding\pickRockOrBanana();
        if ($sample instanceof Scaffolding\Rock && $sample->crush()) {
            // $sample is Scaffolding\Rock here too
        }

        // Short-circuit || narrowing — the right operand of || runs only
        // when the left is false, so `!$guard instanceof Scaffolding\Rock` being false
        // means $guard IS Scaffolding\Rock in the right operand.
        $guard = Scaffolding\pickRockOrBanana();
        if (!$guard instanceof Scaffolding\Rock || !$guard->crush()) {
            // guard clause body
        }

        // The same holds when the subject's declared type names no class:
        // `object` is subsumed by the checked class and `string` is ruled
        // out by the check succeeding, so nothing but Scaffolding\Rock
        // survives into the right operand.
        $looked = Scaffolding\lookUpSpecimen(true);           // object|string
        if (!$looked instanceof Scaffolding\Rock || !Scaffolding\crushOneRock($looked)) {
            // guard clause body
        }

        // A boolean holding the result of an instanceof check carries the
        // check with it: testing the boolean narrows the original subject.
        $stored = Scaffolding\pickRockOrBanana();
        $isRock = $stored instanceof Scaffolding\Rock;
        if ($isRock) {
            $stored->crush();                     // narrowed to Scaffolding\Rock via $isRock
        }
        echo $isRock ? $stored->crush() : $stored->peel();   // both branches narrowed

        // The value a ternary arm *yields* is narrowed too, not just the
        // members reachable inside it.
        $chosen = $isRock ? $stored : new Scaffolding\Rock();
        Scaffolding\crushOneRock($chosen);        // Scaffolding\Rock, from the $isRock arm

        // A boolean holding an `||` chain stands for the whole disjunction:
        // the subject is one of the classes it lists.
        $shelved = Scaffolding\pickRockOrBanana();
        $isKnown = $shelved instanceof Scaffolding\Rock || $shelved instanceof Scaffolding\Banana;
        if ($isKnown) {
            $shelved->weigh();                    // Scaffolding\Rock|Scaffolding\Banana
        }

        // A variable a guarded branch fills stands for the guard itself:
        // reaching the `!== null` test means the branch ran, so whatever
        // it narrowed is narrowed again.
        $weighed = Scaffolding\pickRockOrBanana();
        $label = null;
        if ($weighed instanceof Scaffolding\Rock) {
            $label = new Scaffolding\SpecimenLabel('rock');
        }
        if ($label !== null) {
            Scaffolding\crushOneRock($weighed);    // Scaffolding\Rock, proven by $label
        }

        // The negated form works as a guard clause too.
        $held = Scaffolding\pickRockOrBanana();
        $isBanana = $held instanceof Scaffolding\Banana;
        if (!$isBanana) {
            return;
        }
        $held->peel();                            // narrowed to Scaffolding\Banana after the guard
    }

    public function guardClause(): void
    {
        // Guard-clause form of the same union: after the throw, only the
        // class survives — the array and null alternatives are gone.
        $picked = Scaffolding\pickRockOrRocks(true);          // Scaffolding\Rock|array<Scaffolding\Rock>|null
        if (!$picked instanceof Scaffolding\Rock) {
            throw new \RuntimeException('expected one rock');
        }
        Scaffolding\crushOneRock($picked);                    // narrowed to Scaffolding\Rock
    }
}


// ── Compound & Non-Variable-Subject Narrowing ──────────────────────────────
// instanceof / assert narrowing survives compound && / || conditions and
// applies to property, array-indexed, and inline-assignment subjects, not
// just a single negated variable guard.

class CompoundNarrowingDemo
{
    public function demo(Scaffolding\SpecimenHolder $holder): void
    {
        // && chain: a later conjunct (and the body) see the narrowing
        // established by the first conjunct.
        if ($holder->item instanceof Scaffolding\Rock && $holder->item->crush() === 'smash!') {
            $holder->item->crush();               // property narrowed to Scaffolding\Rock
        }

        // Heterogeneous || guard clause: De Morgan narrows the property
        // subject after the early return.
        if (!$holder instanceof Scaffolding\SpecimenHolder || !$holder->item instanceof Scaffolding\Rock) {
            return;
        }
        $holder->item->crush();                   // property narrowed to Scaffolding\Rock

        // Inline assignment in the condition narrows the assigned variable.
        if (($picked = $holder->maybe()) instanceof Scaffolding\Banana) {
            $picked->peel();                      // inline-assigned var narrowed
        }

        // A pair of `!` cancels, so a doubly negated guard proves exactly
        // what the bare one does. Blade's `@unless (!$specimen)` compiles
        // to this shape, which is how it turns up without being written.
        $specimen = $holder->maybe();             // Scaffolding\Rock|Scaffolding\Banana|null
        if (!(!$specimen)) {
            $specimen->weigh();                   // null dropped, as with `if ($specimen)`
        }

        // The fold applies per conjunct, so wrapping one of them costs the
        // chain nothing.
        if (!(!$specimen) && !(!($specimen instanceof Scaffolding\Rock))) {
            $specimen->crush();                   // narrowed to Scaffolding\Rock
        }
    }

    /**
     * A check on a call narrows the call itself, so the same call
     * written again inside the branch resolves to what the check proved
     * rather than to the declared return type.  The arguments are part
     * of what makes two occurrences the same call, so `lookUp('rock')`
     * and `lookUp('banana')` are narrowed apart from one another.
     */
    public function repeatedCall(Scaffolding\SpecimenHolder $holder): string
    {
        if ($holder->maybe() instanceof Scaffolding\Rock) {
            return $holder->maybe()->crush();     // call narrowed to Scaffolding\Rock
        }

        if ($holder->lookUp('rock') instanceof Scaffolding\Rock) {
            return $holder->lookUp('rock')->crush();   // narrowed to Scaffolding\Rock
        }

        // Try: put the cursor after the `->` below.  Nothing has been
        // proved about this call, so it still offers `Rock|Banana|null`.
        // $holder->lookUp('banana')->

        return '';
    }

    /**
     * A ternary condition proves the same thing an `if` condition does,
     * and its arms are where the repeated call gets written: the
     * re-check-and-reuse idiom for a call that reports "nothing here"
     * with a value rather than an exception.
     */
    public function repeatedCallInTernary(Scaffolding\SpecimenHolder $holder): string
    {
        // The true arm re-evaluates the same call, and the check has
        // already ruled out the `null` it could have answered, so the
        // result satisfies a parameter that does not accept `null`.
        $found = $holder->lookUp('rock') !== null
            ? $holder->lookUp('rock')             // narrowed to Scaffolding\Rock|Banana
            : new Scaffolding\Rock();

        // The else arm of the negated spelling carries the same proof.
        $weight = $holder->lookUp('rock') === null
            ? 0.0
            : $holder->lookUp('rock')->weigh();   // narrowed, so `weigh()` resolves

        return $holder->labelFor($found)->render() . $weight;
    }

    /**
     * A proof lasts as long as the state it was made about.  What a later
     * call on the same receiver does to it is read off the declaration:
     * `@phpstan-pure` keeps it, a method that hands nothing back was called
     * for its effect and drops it, and `@impure` drops it even though the
     * call returns a value.  Anything else computes a value and keeps it.
     */
    public function callInvalidation(Scaffolding\SpecimenHolder $holder): string
    {
        if ($holder->lookUp('rock') instanceof Scaffolding\Rock) {
            $label = $holder->shelfLabel();       // pure — the proof stands
            $count = $holder->shelfCount();       // returns a value — so does this
            $rock = $holder->lookUp('rock');      // still Scaffolding\Rock
            $rock->crush();

            $holder->restock();                   // returns nothing — the shelf moved on
            // Try: put the cursor after the `->` below.  The call is back to
            // `Rock|Banana|null`, so `crush()` is no longer offered alone.
            // $holder->lookUp('rock')->

            return $label . $count;
        }

        if ($holder->lookUp('rock') instanceof Scaffolding\Rock) {
            $holder->rotate();                    // @impure — same effect on the proof
            // Try: put the cursor after the `->` below.  Same widened call.
            // $holder->lookUp('rock')->
        }

        return '';
    }

    /** @param array<Scaffolding\Rock|Scaffolding\Banana> $items */
    public function indexed(array $items): void
    {
        // Integer-indexed subject narrowed by a guard clause.
        if (!$items[0] instanceof Scaffolding\Rock) {
            return;
        }
        $items[0]->crush();                       // element narrowed to Scaffolding\Rock
    }

    /** @param array<string, mixed> $bag */
    public function untypedIndexed(array $bag): void
    {
        // The element type is unknown (mixed), but a guard clause still
        // narrows the array-index subject to the checked class.
        if (!$bag['specimen'] instanceof Scaffolding\Rock) {
            return;
        }
        $bag['specimen']->crush();                // mixed index narrowed to Scaffolding\Rock
    }

    public function arrow(): callable
    {
        // An untyped arrow-function parameter narrowed by an earlier `&&`
        // conjunct is visible to the member access in a later conjunct.
        return fn($specimen) => $specimen instanceof Scaffolding\Rock && $specimen->crush() === 'smash!';
    }

    /**
     * Two checks that both hold prove the subject is both types at once,
     * so it satisfies a parameter naming either one on its own.
     */
    public function bothAtOnce(object $specimen): string
    {
        if ($specimen instanceof Scaffolding\Rock && $specimen instanceof Scaffolding\Labelled) {
            $this->takesRock($specimen);          // Scaffolding\Rock&Scaffolding\Labelled satisfies Scaffolding\Rock
            $this->takesLabelled($specimen);      // ... and Scaffolding\Labelled
            return $specimen->crush() . ':' . $specimen->label();
        }

        return '';
    }

    /**
     * `assert()` reaches the same conclusion about a value already typed as
     * a class it does not nominally implement, which is how a test doubles
     * a concrete dependency with a mock.
     */
    public function assertedBoth(Scaffolding\Rock $rock): string
    {
        assert($rock instanceof Scaffolding\Labelled);
        $this->takesRock($rock);                  // still a Scaffolding\Rock
        $this->takesLabelled($rock);              // and now a Scaffolding\Labelled too

        return $rock->crush() . ':' . $rock->label();
    }

    private function takesRock(Scaffolding\Rock $rock): void {}

    private function takesLabelled(Scaffolding\Labelled $labelled): void {}
}


// ── `match (true)` Arm Narrowing ───────────────────────────────────────────
// A `match (true)` arm's condition proves inside the arm's result exactly
// what the equivalent `if` proves inside its body, and it proves it for
// whatever the result builds, not just for a value it names directly.
// Reaching a later arm means every arm above it was tested and failed, so
// the `default` reads what their conditions ruled out.

class MatchArmNarrowingDemo
{
    public function describe(Scaffolding\SpecimenHolder $holder, string $name): string
    {
        $specimen = $holder->lookUp($name);       // Scaffolding\Rock|Scaffolding\Banana|null
        return match (true) {
            $specimen instanceof Scaffolding\Rock => $specimen->crush(),
            $specimen !== null => (string) $specimen->weigh(),   // null ruled out by this arm
            default => 'nothing',
        };
    }

    /**
     * The arm's proof reaches the elements of an array it builds, so this
     * is a list of plain floats rather than of nullable ones.
     *
     * @return list<float>
     */
    public function weights(Scaffolding\SpecimenHolder $holder): array
    {
        $first = $holder->lookUp('rock');         // Scaffolding\Rock|Scaffolding\Banana|null
        $second = $holder->lookUp('banana');      // same, and this one is null

        return match (true) {
            $first !== null && $second !== null => [$first->weigh(), $second->weigh()],
            $first !== null => [$first->weigh()],
            default => [],
        };
    }

    /**
     * An arm listing several conditions runs when any one of them matched,
     * so only what all of them prove holds inside it: here both name the
     * same class, and neither says anything about `$other`.
     */
    public function eitherCondition(Scaffolding\SpecimenHolder $holder): string
    {
        $specimen = $holder->lookUp('rock');
        $other = $holder->lookUp('banana');

        return match (true) {
            $specimen instanceof Scaffolding\Rock,
            $specimen instanceof Scaffolding\Rock && $other !== null => $specimen->crush(),
            default => 'nothing',
        };
    }

    /** The `default` sees the inverse of every arm above it. */
    public function fallback(Scaffolding\SpecimenHolder $holder): float
    {
        $specimen = $holder->lookUp('rock');

        return match (true) {
            $specimen === null => 0.0,
            default => $specimen->weigh(),        // the arm above ruled the null out
        };
    }
}


// ── A `?->` Chain Compared to a Value That Cannot Be Null ───────────────────
// A chain that short-circuited holds `null`, and `null` is never identical
// to a value whose type excludes it.  So the comparison holding proves the
// chain ran, which is a proof about every receiver it would have
// short-circuited on, including through the plain `->` links written after
// the `?->`.

class NullsafeComparisonDemo
{
    public function sameWeight(Scaffolding\SpecimenHolder $holder): string
    {
        $specimen = $holder->lookUp('rock');      // Scaffolding\Rock|Scaffolding\Banana|null

        if ($specimen?->weigh() === $holder->item->weigh()) {
            return (string) $specimen->weigh();   // null dropped: the chain must have run
        }

        // The `!==` spelling proves the same thing where it fails.
        if ($specimen?->weigh() !== $holder->item->weigh()) {
            return 'different';
        }

        return (string) $specimen->weigh();       // null dropped past the guard too
    }

    /**
     * Nothing is proved when the other side can be null as well: both
     * sides holding `null` is one of the ways the comparison succeeds.
     */
    public function bothMayBeNull(Scaffolding\SpecimenHolder $holder): string
    {
        $specimen = $holder->lookUp('rock');

        if ($specimen?->weigh() === $holder->lookUp('banana')?->weigh()) {
            // Try: put the cursor after the `?->` below.  `$specimen` is
            // still `Scaffolding\Rock|Scaffolding\Banana|null` here.
            return (string) $specimen?->weigh();
        }

        return 'different';
    }
}


// ── Variables Filled Together in One Branch Share Their Null ────────────────
// `$label` is written on exactly the path that leaves `$specimen` holding a
// value, so the two are null together or not at all.  The merge at the end of
// the branch records that, which is what lets a later check on one of them
// settle the other, even though the check never names it.

class CorrelatedNullDemo
{
    public function describe(Scaffolding\SpecimenHolder $holder, string $name): string
    {
        $label = null;
        $specimen = null;

        if ($name !== '') {
            $specimen = $holder->lookUp($name);
            if ($specimen !== null) {
                $label = $holder->labelFor($specimen);
            }
        }

        if ($specimen !== null) {
            // Try: put the cursor after the `->` below.  `$label` is a
            // `Scaffolding\SpecimenLabel` here rather than a nullable one:
            // the only path that leaves `$specimen` holding a value is the
            // one that filled `$label` too.
            return $label->render();
        }

        return 'nothing on the shelf';
    }

    /**
     * Two variables filled under conditions of their own are not a pair,
     * however alike the branches look, so `$label` keeps its `null` here.
     */
    public function unrelated(bool $wantSpecimen, bool $wantLabel): string
    {
        $label = null;
        $specimen = null;

        if ($wantSpecimen) {
            $specimen = new Scaffolding\Rock();
        }
        if ($wantLabel) {
            $label = new Scaffolding\SpecimenLabel('1kg');
        }

        if ($specimen !== null) {
            // `$label` is still `Scaffolding\SpecimenLabel|null` here, which
            // is why the `?->` is the only safe way to read it.
            return $label?->render() ?? 'unlabelled';
        }

        return 'nothing on the shelf';
    }
}


// ── Scalar Guards in Compound Conditions and Ternaries ──────────────────────
// The `is_*` family, null checks, and comparisons narrow wherever they are
// written: each operand of a negated `||` guard, both arms of a ternary in any
// position, and an assignment made inside an `elseif` condition.

class ScalarGuardNarrowingDemo
{
    /**
     * Falling through `if (!guard1 || !guard2) { return; }` means every
     * operand was false, so each one's inverse holds afterwards.
     */
    public function rejectEverythingElse(string|array|null $payload): string
    {
        if (!is_string($payload) || $payload === '') {
            return 'skipped';
        }

        // `!is_string` ruled out the array and null halves, and `=== ''`
        // refines what is left to non-empty-string.
        return Scaffolding\takesNonEmptyString($payload);      // non-empty-string
    }

    /** Each arm of a ternary is resolved under its own polarity. */
    public function eitherArm(string|array|null $period): string
    {
        $chosen = is_string($period) ? $period : 'today';

        // The same ternary in argument position narrows identically.
        return Scaffolding\takesGradeString(is_string($period) ? $period : $chosen);
    }

    /** A nested ternary's else arm carries the outer condition's inverse. */
    public function nested(string|int|null $value): string
    {
        return is_string($value)
            ? $value                               // string
            : (is_int($value) ? (string) $value : 'none');
    }

    /**
     * A truthy check rules out every value that can only ever be false,
     * not just `null` and `false`.
     *
     * The `?? []` is the usual way a union like this appears: the default
     * is not of the value's own type, so what comes out spans both. An
     * empty array is false in PHP, so the guard below leaves only the
     * string half and `explode()` is handed what it asks for.
     *
     * @return list<string>
     */
    public function onlyWhatCanBeTrue(): array
    {
        $markets = Scaffolding\scaffoldingReadSetting('markets') ?? [];

        if ($markets) {
            return explode(',', $markets);         // string, no array{} left
        }

        return [];
    }

    /**
     * What is only *sometimes* false is kept: a plain `string` still holds
     * `''`, so the guard narrows it no further than the declaration does.
     */
    public function whatMayStillBeEither(string $label): string
    {
        if ($label) {
            return Scaffolding\takesGradeString($label);   // still string
        }

        return 'unlabelled';
    }

    /** An assignment in an `elseif` condition narrows what it wrote. */
    public function fromElseif(bool $skip, string|array|null $raw): string
    {
        if ($skip) {
            return 'skipped';
        } elseif ($found = Scaffolding\scaffoldingReadPayload($raw)) {
            return Scaffolding\takesStringOrArray($found);     // string|array, never null
        }

        return 'empty';
    }

    /**
     * A key expression keeps its own domain, so a map built under `string`
     * keys really is `array<string, string>` rather than `array<int|string, …>`.
     *
     * @param  list<Scaffolding\LabelledRock>  $specimens
     * @return array<string, string>
     */
    public function labelMap(array $specimens): array
    {
        $labels = [];
        foreach ($specimens as $specimen) {
            $labels[$specimen->label()] = $specimen->crush();
        }

        return $labels;                            // array<string, string>
    }

    /**
     * An `int` counter used as a write key stays an `int` key, whether it
     * steps before or after the read.
     *
     * @return array<int, string>
     */
    public function numberedLines(): array
    {
        $line = 0;
        $numbered = [];
        foreach (['first', 'second'] as $text) {
            $numbered[++$line] = $text;
        }

        return $numbered;                          // array<int, string>
    }
}


// ── Discriminating-Property Narrowing ───────────────────────────────────────
// A union of object types is narrowed by a check on a property only some of
// its members could have passed, the way TypeScript discriminates on a tag.

class PropertyDiscriminantDemo
{
    /**
     * A type guard on the property rules out the member that declares that
     * property with an incompatible type.
     */
    public function byTypeGuard(): int
    {
        $tag = Scaffolding\pickTag();                         // Scaffolding\TextTag|Scaffolding\NumberTag
        if (is_string($tag->tag)) {
            return $tag->letters();               // narrowed to Scaffolding\TextTag
            // MUST NOT appear: digits() (Scaffolding\NumberTag only)
        }

        return $tag->digits();                    // narrowed to Scaffolding\NumberTag (else branch)
    }

    /**
     * A tagged union: each member pins its tag to one value, so comparing
     * against that value picks the member out.
     */
    public function byTagValue(): float
    {
        $sample = Scaffolding\pickWeighing();                 // Scaffolding\Weighed|Scaffolding\Unweighed
        if ($sample->state === 'weighed') {
            return $sample->grams;                // narrowed to Scaffolding\Weighed
            // MUST NOT appear: reason() (Scaffolding\Unweighed only)
        }

        return 0.0;
    }
}


// ── property_exists() / method_exists() Narrowing ───────────────────────────
// A member-existence guard proves the (otherwise unknown) member for the rest
// of the branch, mirroring PHPStan's `object&hasProperty(...)` intersection.
// The proof is confined to the guarded branch: the same access outside it is
// still flagged.

class MemberExistsNarrowingDemo
{
    public function property(Scaffolding\ApiResponse $response): ?string
    {
        // property_exists() proves the dynamically-populated property, so the
        // access resolves instead of reporting an unknown member.
        if (property_exists($response, 'errorMessage') && is_string($response->errorMessage)) {
            return $response->errorMessage;       // proven by property_exists
        }
        return null;
    }

    public function guardClause(Scaffolding\ApiResponse $response): string
    {
        // Negated guard clause: after the early return the property exists.
        if (!property_exists($response, 'detail')) {
            return 'none';
        }
        return (string) $response->detail;        // proven after the guard
    }

    public function issetGuard(Scaffolding\ApiResponse $response): ?string
    {
        // isset($obj->prop) proves the property exists (and is non-null)
        // for the guarded branch, exactly like property_exists().
        if (isset($response->errorMessage)) {
            return (string) $response->errorMessage; // proven by isset
        }
        return null;
    }

    public function ternary(Scaffolding\ApiResponse $response): string
    {
        // Both member-existence guards prove the property in a ternary's
        // then-branch, mirroring the if-statement form.
        $viaPropertyExists = property_exists($response, 'detail')
            ? (string) $response->detail          // proven by property_exists
            : 'none';
        $viaIsset = isset($response->errorMessage)
            ? (string) $response->errorMessage    // proven by isset
            : 'none';
        return $viaPropertyExists . '|' . $viaIsset;
    }

    public function method(Scaffolding\DynamicHandler $handler): void
    {
        // method_exists() proves the method for the guarded branch.
        if (method_exists($handler, 'customHook')) {
            $handler->customHook();               // proven by method_exists
        }
    }
}


// ── Type Guard Narrowing (is_array, is_object, …) ──────────────────────────

class TypeGuardNarrowingDemo
{
    /**
     * @param null|list<Scaffolding\Pen>|Scaffolding\Pen $input
     */
    public function demo(null|array|Scaffolding\Pen $input): void
    {
        // is_array() narrows the union to the array-like PHPDoc member,
        // preserving the generic element type for foreach iteration.
        if (is_array($input)) {
            foreach ($input as $pen) {
                $pen->write();                    // list<Scaffolding\Pen> → Scaffolding\Pen
            }
        }

        // Else branch: non-array members survive
        if (is_array($input)) {
            // array branch
        } else {
            // $input is null|Scaffolding\Pen here
        }

        // Guard clause: is_array() + early return
        if (is_array($input)) {
            return;
        }
        // $input is null|Scaffolding\Pen after the guard

        // is_object() narrows to class members only
        $mixed = Scaffolding\pickRockOrBanana();              // Scaffolding\Rock|Scaffolding\Banana
        if (is_object($mixed)) {
            $mixed->weigh();                      // both Scaffolding\Rock and Scaffolding\Banana have weigh()
        }

        // is_object() narrows mixed → object, suppressing diagnostics
        // on dynamic property access (stdClass / object permit any property).
        $decoded = json_decode('{}');             // mixed
        if (is_object($decoded)) {
            echo $decoded->anything;              // no diagnostic — object allows any property
        }

        // Compound && condition: is_object() narrowing propagates
        // through the entire condition and into the if-body.
        $payload = json_decode('{}');             // mixed
        if (is_object($payload) && property_exists($payload, 'name')) {
            echo $payload->name;                  // no diagnostic
        }

        // A property assigned `new stdClass()` resolves back to stdClass,
        // so a nested object graph built up field by field type-checks.
        $settings = new \stdClass();
        $settings->cache = new \stdClass();       // $settings->cache : stdClass
        $settings->cache->ttl = 3600;             // no diagnostic on ->ttl
    }

    /**
     * `is_iterable()` keeps the members `foreach` can walk: an array in
     * any of its spellings, and an object whose interfaces reach
     * \Traversable. A plain object goes out with the scalars.
     */
    public function iterableGuard(
        Scaffolding\ItemIterableCollection|Scaffolding\Pen|string $value
    ): void {
        if (is_iterable($value)) {
            foreach ($value as $item) {
                $item->write();               // IteratorAggregate<int, Pen> → Scaffolding\Pen
            }
            return;
        }
        // $value is Scaffolding\Pen|string here — the collection passed the guard
        if (is_string($value)) {
            return;
        }
        $value->write();                      // Scaffolding\Pen is all that is left
    }

    /**
     * An `array<T>|false` return keeps its element type after the false
     * check, so the surviving array still iterates to the element type.
     */
    public function falseUnion(): void
    {
        $pens = Scaffolding\loadPensOrFail();                  // array<int, Scaffolding\Pen>|false
        if (!is_array($pens)) {
            return;
        }
        foreach ($pens as $pen) {
            $pen->write();                        // array<int, Scaffolding\Pen> → Scaffolding\Pen
        }

        // The `=== false` guard narrows the same way.
        $more = Scaffolding\loadPensOrFail();                  // array<int, Scaffolding\Pen>|false
        if ($more === false) {
            return;
        }
        foreach ($more as $pen) {
            $pen->write();                        // array<int, Scaffolding\Pen> → Scaffolding\Pen
        }

        // `assert()` is the same guard written as one statement: it proves
        // its condition for everything below it, so the sentinel is gone.
        $asserted = Scaffolding\loadPensOrFail();              // array<int, Scaffolding\Pen>|false
        assert($asserted !== false);
        foreach ($asserted as $pen) {
            $pen->write();                        // array<int, Scaffolding\Pen> → Scaffolding\Pen
        }
    }
}


// ── Scalar Guard Clauses on a Property ─────────────────────────────────────
//
// A check that rules out a scalar member of a union has no class to swap, so
// it narrows the property path itself. Every guard shape that narrows a local
// narrows a property the same way, however the guarded branch ends.

class PropertyGuardDemo
{
    public string|false $label = false;

    public ?Scaffolding\ScaffoldingHandle $handle = null;

    public function earlyReturn(): string
    {
        if ($this->label === false) {
            return 'unlabelled';
        }
        return strtoupper($this->label);          // string after the guard
    }

    public function earlyThrow(): string
    {
        if (!$this->label) {                      // `!` names the property too
            throw new \RuntimeException('no label');
        }
        return strtoupper($this->label);          // string after the guard
    }

    public function skipIteration(): int
    {
        $width = 0;
        foreach ([1, 2, 3] as $_) {
            if (empty($this->label)) {            // empty() names it as well
                continue;
            }
            $width += strlen($this->label);       // string on the surviving path
        }
        return $width;
    }

    public function chainedPath(): string
    {
        // The path can be as deep as it needs to be.
        if ($this->handle === null) {
            return 'anonymous';
        }
        if ($this->handle->name === false) {
            return 'anonymous';
        }
        return strtoupper($this->handle->name);   // string after the guard
    }

    public function rewritten(string|false $next): string
    {
        if ($this->label === false) {
            return 'unlabelled';
        }
        // A write replaces what the guard proved rather than outliving it,
        // so `$this->label` is `string|false` again and needs guarding anew.
        $this->label = $next;
        if ($this->label === false) {
            return 'unlabelled';
        }
        return strtoupper($this->label);          // string after the second guard
    }
}


// ── instanceof self/static/parent Narrowing ────────────────────────────────

class InstanceofSelfDemo extends Scaffolding\ScaffoldingSedan
{
    public function sport(): void {}

    public function demo(Scaffolding\ScaffoldingMotor $m): void
    {
        // instanceof self — narrows to InstanceofSelfDemo
        assert($m instanceof self);
        $m->cruise();                             // inherited from Scaffolding\ScaffoldingSedan
        $m->sport();                              // own method via self narrowing

        // instanceof static — narrows to InstanceofSelfDemo
        $x = Scaffolding\getUnknownValue();
        if ($x instanceof static) {
            $x->sport();                          // narrowed to static (this class)
        }

        // instanceof parent — narrows to Scaffolding\ScaffoldingSedan
        $y = Scaffolding\getUnknownValue();
        if ($y instanceof parent) {
            $y->cruise();                         // narrowed to parent (Scaffolding\ScaffoldingSedan)
        }
    }
}


// ── Negated Disjunction and Subclass Subtraction ────────────────────────────

class NegatedDisjunctionDemo
{
    /**
     * A guard that rules out two types at once leaves the value narrowed to
     * both of them, and a check that fails rules out the subclasses of the
     * class it names along with the class itself.
     */
    public function demo(Scaffolding\ScaffoldingMotor $motor): string
    {
        // Everything that is neither type leaves here, so `$motor` is
        // Scaffolding\ScaffoldingSedan|Scaffolding\ScaffoldingCoupe below.
        if (!($motor instanceof Scaffolding\ScaffoldingSedan
            || $motor instanceof Scaffolding\ScaffoldingCoupe)) {
            return 'other';
        }
        // Try: completion here offers start() from the base class of both.
        $motor->start();

        if (!$motor instanceof Scaffolding\ScaffoldingSedan) {
            return $motor->race();                // Scaffolding\ScaffoldingCoupe
        }
        $motor->cruise();                         // Scaffolding\ScaffoldingSedan
        return 'sedan';
    }

    /**
     * A subclass passes a check on its parent and stays as itself, so
     * launch() is still reachable inside the branch.
     *
     * @param Scaffolding\ScaffoldingSportSedan|Scaffolding\ScaffoldingCoupe $motor
     */
    public function keepsSubclass($motor): string
    {
        if ($motor instanceof Scaffolding\ScaffoldingSedan) {
            return $motor->launch();              // Scaffolding\ScaffoldingSportSedan
        }
        // A ScaffoldingSportSedan is a ScaffoldingSedan, so the failed check
        // ruled it out here too and only the coupe is left.
        return $motor->race();                    // Scaffolding\ScaffoldingCoupe
    }

    /**
     * A disjunction proves that one of its legs held, not that any
     * particular one did. Which is why a check standing beside an
     * unrelated operand narrows nothing, while two checks on the same
     * subject leave the union of what they name.
     */
    public function provesOneLegOnly(Scaffolding\ScaffoldingMotor $motor, bool $flag): string
    {
        if ($motor instanceof Scaffolding\ScaffoldingCoupe || $flag) {
            // The flag gets in on its own, so `$motor` is still the
            // Scaffolding\ScaffoldingMotor it was on the way in and race()
            // is not offered here.
            $motor->start();                      // Scaffolding\ScaffoldingMotor
        }

        if ($motor instanceof Scaffolding\ScaffoldingSedan
            || $motor instanceof Scaffolding\ScaffoldingCoupe) {
            // Try: completion here offers start(), which both legs share.
            $motor->start();                      // Sedan|Coupe
        }

        return 'motor';
    }
}


// ── Custom Assert Narrowing ─────────────────────────────────────────────────

class AssertNarrowingDemo
{
    public function demo(): void
    {
        $unknown = Scaffolding\getUnknownValue();
        Scaffolding\assertRock($unknown);                     // @phpstan-assert Scaffolding\Rock $value
        $unknown->crush();

        $sample = Scaffolding\pickRockOrBanana();
        if (Scaffolding\isRock($sample)) {                    // @phpstan-assert-if-true Scaffolding\Rock
            $sample->crush();
        } else {
            $sample->peel();
        }

        $maybe = Scaffolding\pickRockOrBanana();
        if (Scaffolding\isNotRock($maybe)) {                  // @phpstan-assert-if-false Scaffolding\Rock
            $maybe->peel();
        } else {
            $maybe->crush();
        }
    }

    // An asserted type written as a union rules out every member, so the
    // shape Laravel's `filled()` carries (`!=null|''`) strips the null.
    public function unionAssert(?string $search): string
    {
        if (Scaffolding\demoFilled($search)) {
            return $search;                                   // string, not ?string
        }

        return '';
    }
}


// ── Static Method Assert Narrowing ─────────────────────────────────────────

class StaticAssertNarrowingDemo
{
    public function demo(): void
    {
        // @phpstan-assert on static method — unconditional narrowing
        $unknown = Scaffolding\getUnknownValue();
        Scaffolding\StaticAssert::assertRock($unknown);
        $unknown->crush();                        // narrowed to Scaffolding\Rock

        // @phpstan-assert-if-true on static method — narrows in then-branch
        $sample = Scaffolding\pickRockOrBanana();
        if (Scaffolding\StaticAssert::isRock($sample)) {
            $sample->crush();                     // narrowed to Scaffolding\Rock
        }

        // @phpstan-assert-if-false on static method — narrows in else-branch
        $maybe = Scaffolding\pickRockOrBanana();
        if (Scaffolding\StaticAssert::isNotRock($maybe)) {
            $maybe->peel();                       // narrowed to Scaffolding\Banana
        } else {
            $maybe->crush();                      // narrowed to Scaffolding\Rock
        }
    }
}

// ── Inherited Assert Narrowing (PHPUnit shape) ─────────────────────────────

// The assert method is declared on the parent (Scaffolding\StaticAssert); narrowing
// still applies when it is reached through inheritance, exactly like a
// PHPUnit test case calling the inherited `assertInstanceOf`.
class InheritedAssertNarrowingDemo extends Scaffolding\StaticAssert
{
    public function demo(): void
    {
        // $this-> on an inherited assert method
        $viaThis = Scaffolding\getUnknownValue();
        $this->assertRock($viaThis);
        $viaThis->crush();                        // narrowed to Scaffolding\Rock

        // self:: on an inherited assert method
        $viaSelf = Scaffolding\getUnknownValue();
        self::assertRock($viaSelf);
        $viaSelf->crush();                        // narrowed to Scaffolding\Rock

        // static:: on an inherited assert method
        $viaStatic = Scaffolding\getUnknownValue();
        static::assertRock($viaStatic);
        $viaStatic->crush();                      // narrowed to Scaffolding\Rock
    }
}


// ── assertTrue / assertFalse Re-Export (PHPUnit shape) ─────────────────────

// PHPUnit's assertTrue()/assertFalse() carry `@phpstan-assert true/false
// $condition`, so wrapping a check in one re-exports that condition exactly
// like the bare `if` form.  assertIsObject() first narrows the mixed value
// to `object`.
class AssertConditionReexportDemo extends Scaffolding\StaticAssert
{
    public function demo(): void
    {
        // assertIsObject() narrows mixed → object; assertTrue(property_exists())
        // then proves the dynamically-populated property.
        $model = Scaffolding\getUnknownValue();               // mixed
        self::assertIsObject($model);             // narrowed to object
        self::assertTrue(property_exists($model, 'value'));
        echo $model->value;                       // proven by the re-exported guard

        // assertFalse() re-exports the inverse of the condition: ruling out
        // the Scaffolding\Banana branch narrows the union to Scaffolding\Rock.
        $subject = Scaffolding\pickRockOrBanana();            // Scaffolding\Rock|Scaffolding\Banana
        self::assertFalse($subject instanceof Scaffolding\Banana);
        $subject->crush();                        // narrowed to Scaffolding\Rock

        // assertIsNotString() (a `@phpstan-assert !string` guard) drops the
        // string arm of a union, leaving the object.
        $mixedValue = self::pickStringOrRock();   // string|Scaffolding\Rock
        self::assertIsNotString($mixedValue);
        $mixedValue->crush();                     // narrowed to Scaffolding\Rock

        // A pseudo-type the class hierarchy knows nothing about narrows
        // just as well: `resource` and `null` travel the same channel the
        // matching is_resource() / === null check does.
        $handle = Scaffolding\getUnknownValue();  // mixed
        self::assertIsResource($handle);          // narrowed to resource
        $nothing = Scaffolding\getUnknownValue(); // mixed
        self::assertIsNull($nothing);             // narrowed to null
    }

    /** @return string|Scaffolding\Rock */
    private static function pickStringOrRock(): string|Scaffolding\Rock
    {
        return new Scaffolding\Rock();
    }
}


// ── Guard Clause Narrowing (Early Return / Throw) ──────────────────────────

class GuardClauseDemo
{
    public function demo(): void
    {
        $subject = Scaffolding\pickRockOrBanana();            // Scaffolding\Rock|Scaffolding\Banana
        if (!$subject instanceof Scaffolding\Banana) {
            return;                               // early return — guard clause
        }
        $subject->peel();                         // narrowed to Scaffolding\Banana after guard

        $candidate = Scaffolding\pickRockOrBanana();          // Scaffolding\Rock|Scaffolding\Banana
        if ($candidate instanceof Scaffolding\Rock) {
            throw new Exception('no rocks');       // early throw — guard clause
        }
        $candidate->peel();                       // narrowed to Scaffolding\Banana (Scaffolding\Rock excluded)

        $unknown = Scaffolding\getUnknownValue();
        if (!$unknown instanceof Scaffolding\Rock) return;    // single-statement guard (no braces)
        $unknown->crush();                        // narrowed to Scaffolding\Rock
    }

    /**
     * A guard body whose only statement returns `never` cannot fall
     * through, so it terminates the branch just like `return` does.
     * The receiver does not have to be a variable: the helper call's
     * return type says which class `fail()` is looked up on.
     */
    public function neverGuard(): void
    {
        $produce = Scaffolding\pickRockOrBanana();            // Scaffolding\Rock|Scaffolding\Banana
        if (!$produce instanceof Scaffolding\Banana) {
            Scaffolding\demoAborter()->fail('not a banana');  // : never — exits here
        }
        $produce->peel();                         // narrowed to Scaffolding\Banana after guard
    }

    /** Positive instanceof + early return on a mixed parameter. */
    public function mixedGuard(mixed $value): void
    {
        if ($value instanceof Scaffolding\Banana) {
            return;                               // $value is Scaffolding\Banana → exit
        }
        // After the guard, $value is NOT Scaffolding\Banana.
        if ($value instanceof Scaffolding\Rock) {
            $value->crush();                      // narrowed to Scaffolding\Rock (not Scaffolding\Banana)
        }
    }
}


// ── in_array Strict-Mode Narrowing ─────────────────────────────────────────

class InArrayNarrowingDemo
{
    /**
     * @param Scaffolding\Rock|Scaffolding\Banana $item
     * @param list<Scaffolding\Rock> $rocks
     */
    public function demo($item, array $rocks): void
    {
        if (in_array($item, $rocks, true)) {
            $item->crush();                       // narrowed to Scaffolding\Rock
            // MUST NOT appear: peel() (Scaffolding\Banana only)
        } else {
            $item->peel();                        // excluded Scaffolding\Rock → Scaffolding\Banana
            // MUST NOT appear: crush() (Scaffolding\Rock only)
        }

        // Guard clause with in_array
        $specimen = Scaffolding\pickRockOrBanana();           // Scaffolding\Rock|Scaffolding\Banana
        if (!in_array($specimen, $rocks, true)) {
            return;
        }
        $specimen->crush();                       // narrowed to Scaffolding\Rock after guard
    }

    /** A constant list of literals narrows a scalar needle to its values. */
    public function gate(?string $grade): string
    {
        if (!in_array($grade, Scaffolding\GRADES, true)) {
            throw new \RuntimeException('unknown grade');
        }

        // $grade is 'a'|'b'|'c' here, so the null the parameter allowed is
        // gone and the return type below is satisfied.
        return $grade;
    }
}


// ── Generics (@template / @extends) ────────────────────────────────────────

class GenericsDemo
{
    public function demo(): void
    {
        $repo = new Scaffolding\PenRepository();
        $repo->find(1)->write();                  // Scaffolding\Repository<Scaffolding\Pen>::find() → Scaffolding\Pen
        $repo->findOrNull(1)?->write();           // ?Scaffolding\Pen

        $pens = new Scaffolding\PenCollection();              // Scaffolding\TypedCollection<int, Scaffolding\Pen>
        $pens->first()->write();
        // MUST NOT appear: refill() (private on Scaffolding\Pen)
        $pens->thickOnly();                       // own method on subclass

        $cachingRepo = new Scaffolding\CachingPenRepository();
        $cachingRepo->find(1)->write();           // grandparent generics

        $responses = new Scaffolding\ResponseCollection();    // @phpstan-extends variant
        $responses->first()->getStatusCode();
    }
}


// ── Bare Generic Class Names (@template bound) ─────────────────────────────

class BareGenericSubjectDemo
{
    /**
     * A subject that names a generic class without its type arguments
     * supplies no `TPen`, so a member typed `TPen` reads as the bound the
     * `@template` declares. Without that, `first()` would resolve to a
     * class called `TPen`, which exists nowhere.
     */
    public function demo(Scaffolding\ScaffoldingBoundedPenCollection $pens): void
    {
        $pens->first()?->write();                 // TPen of Scaffolding\Pen → Scaffolding\Pen
        // MUST NOT appear: highlight() (Scaffolding\Marker's, not Scaffolding\Pen's)

        /** @var Scaffolding\ScaffoldingBoundedPenCollection $viaDocblock */
        $viaDocblock = $pens;
        $viaDocblock->first()?->write();          // same through a @var

        // An unbounded parameter is the widest thing the declaration
        // guarantees, which is `mixed`.
        var_dump((new Scaffolding\ScaffoldingUnboundedBox())->unwrap());
    }
}


// ── Constant Tables Read Through a Type Operator ────────────────────────────

class ConstantTableLookupDemo
{
    /**
     * A constant holding an array literal is as readable an operand as an
     * inline `array{…}` shape, so `key-of<TABLE>` names its keys and
     * `TABLE[K]` names the value under the one key the call site bound.
     */
    public function demo(): void
    {
        // Each call reads as the one entry its key names, not as the
        // `int|string` union the declaration has to spell.
        $width = Scaffolding\scaffoldingToolDefault('width');  // Scaffolding\TOOL_DEFAULTS['width'] → 2
        var_dump($width + 1);

        $ink = Scaffolding\scaffoldingToolDefault('ink');      // Scaffolding\TOOL_DEFAULTS['ink'] → 'black'
        var_dump(strtoupper($ink));

        // An omitted argument binds the template from the parameter's own
        // default, which is as known here as a key written at the call site.
        $fallback = Scaffolding\scaffoldingDefaultToolSetting();  // Scaffolding\TOOL_DEFAULTS['ink'] → 'black'
        var_dump(strtoupper($fallback));

        // The class-constant spelling reads the same way.
        $retries = Scaffolding\ScaffoldingLimits::lookUp('retries');
        var_dump($retries + 1);                    // LIMITS['retries'] → 3

        $label = Scaffolding\ScaffoldingLimits::lookUp('label');
        var_dump(strtoupper($label));              // LIMITS['label'] → 'off'

        // A signature that declares no `@template` reads the constant just
        // the same: the parameter accepts only the table's own keys, and the
        // return is the union of its values.
        var_dump(Scaffolding\scaffoldingToolSettingName('ink'));  // key-of<Scaffolding\TOOL_DEFAULTS> → 'width'|'ink'
        var_dump(Scaffolding\scaffoldingAnyToolDefault());        // value-of<Scaffolding\TOOL_DEFAULTS> → 2|'black'

        // The declaration reads the same way from inside the body, so a
        // `@return` naming the table is held to what the table holds.
        // Try: hover `$setting` inside `Scaffolding\scaffoldingToolDefaultFor()`.
        var_dump(Scaffolding\scaffoldingToolDefaultFor('ink'));   // value-of<Scaffolding\TOOL_DEFAULTS> → 2|'black'
        var_dump(Scaffolding\ScaffoldingLimits::anyLimitName());  // key-of<LIMITS> → 'retries'|'label'
    }
}


// ── @implements Generic Resolution ─────────────────────────────────────────

class ImplementsGenericDemo
{
    public function demo(): void
    {
        $repo = new Scaffolding\PenStorage();
        $repo->find(1)->write();                  // @implements Scaffolding\Storage<Scaffolding\Pen> → Scaffolding\Pen

        $penCatalog = new Scaffolding\PenCatalog();
        $penCatalog->find(1)->write();            // @template-implements alias

        $items = new Scaffolding\ItemIterableCollection();
        foreach ($items as $item) {
            $item->write();                       // @implements IteratorAggregate<Scaffolding\Pen>
        }
    }
}


// ── Built-in Generic Collections (ArrayIterator, SplFixedArray, etc.) ───────

class BuiltinGenericCollectionDemo
{
    /** @return \ArrayIterator<int, Scaffolding\Pen> */
    public function getPens(): \ArrayIterator { return new \ArrayIterator([new Scaffolding\Pen()]); }

    public function demo(): void
    {
        $pen = $this->getPens()->current();
        $pen->write();                            // ArrayIterator<int, Scaffolding\Pen> → current() returns Scaffolding\Pen

        // Direct chain also works:
        $this->getPens()->current()->write();     // same resolution through the chain
    }
}


// ── SimpleXMLElement Iteration (Iterator without generics) ──────────────────

class SimpleXmlIterationDemo
{
    public function demo(): void
    {
        $xml = new \SimpleXMLElement('<root><child/></root>');
        foreach ($xml->children() as $child) {
            $child->getName();                    // Iterator (no generics) → current(): static
        }
    }

    public function firstChild(): ?\SimpleXMLElement
    {
        foreach ((new \SimpleXMLElement('<root><child/></root>'))->children() as $child) {
            return $child;
        }
        return null;
    }
}


// ── SPL Wrapper Iterators (FilterIterator, DirectoryIterator) ───────────────

class SplWrapperIterationDemo
{
    public function demo(): void
    {
        // `Scaffolding\PhpFileFilter` is `@extends FilterIterator<int, SplFileInfo,
        // \Iterator<int, SplFileInfo>>`. The value type is the middle
        // argument (SplFileInfo), not the trailing inner-iterator argument.
        $files = new Scaffolding\PhpFileFilter(new \ArrayIterator([new \SplFileInfo(__FILE__)]));
        foreach ($files as $file) {
            $file->getRealPath();                 // FilterIterator<_, SplFileInfo, _> → SplFileInfo
        }

        // Directly-constructed SPL iterator: the value type comes from
        // `DirectoryIterator::current()`.
        foreach (new \DirectoryIterator(__DIR__) as $entry) {
            $entry->isFile();                     // DirectoryIterator → current(): DirectoryIterator
        }

        // `RecursiveIteratorIterator` says nothing about what it yields on
        // its own: the wrapped iterator does, and the wrapper carries it
        // through. This is the directory-walk idiom.
        $tree = new \RecursiveIteratorIterator(
            new \RecursiveDirectoryIterator(__DIR__, \FilesystemIterator::SKIP_DOTS)
        );
        foreach ($tree as $file) {
            $file->getExtension();                // SplFileInfo from RecursiveDirectoryIterator
        }
    }
}


// ── Inherited Docblock Types ────────────────────────────────────────────────

class InheritedDocblockDemo
{
    public function demo(): void
    {
        // Interface declares @return list<Scaffolding\Pen>, implementor has only `: array`.
        // The richer type propagates automatically.
        $holder = new Scaffolding\ScaffoldingConcreteHolder();
        $holder->getPens()[0]->write();            // list<Scaffolding\Pen> inherited from interface

        // Parent class declares @return list<Scaffolding\Pen>, child overrides with `: array`.
        $child = new Scaffolding\ScaffoldingChildHolder();
        $child->getPens()[0]->write();             // list<Scaffolding\Pen> inherited from parent

        // When the child writes its own @return, it wins over the parent.
        $cat = new Scaffolding\ScaffoldingCatStore();
        $cat->getAnimals()[0]->label();            // list<Scaffolding\Pencil> from child's own docblock

        // Parameter types propagate by position (child may rename params).
        $box = new Scaffolding\ScaffoldingPenBox();
        $box->accept([new Scaffolding\Pen()]);                 // @param list<Scaffolding\Pen> inherited from interface

        // Grandparent @return flows through the entire chain.
        $deep = new Scaffolding\ScaffoldingDeepChild();
        $deep->getPens()[0]->write();              // list<Scaffolding\Pen> from grandparent

        // The implementor's own `: array` outranks the wider interface
        // docblock: the `string` half cannot survive that declaration.
        $payload = (new Scaffolding\ScaffoldingArrayPayload())->payload();
        $payload;                                  // array<string, mixed>|list<mixed>
    }
}


// ── Boolean Literals and Truthiness ─────────────────────────────────────────

class BooleanLiteralDemo
{
    public function demo(string $raw, string $format): string
    {
        // `false` keeps its own type, so the truthiness check below has
        // something to subtract instead of an unfalsifiable `bool`.
        $time = false;
        if ($raw !== '') {
            $time = strtotime($raw);               // int|false
        }
        if ($time) {
            return date($format, $time);           // int — the false half is gone
        }

        // A value that genuinely is `bool` narrows to `true` in the branch
        // the check proved.
        $found = false;
        foreach ([1, 2, 3] as $item) {
            if ($item === 2) {
                $found = true;
                break;
            }
        }
        if ($found) {
            $found;                                // true
        }
        return '';
    }
}


// ── Closure Literal Signatures ──────────────────────────────────────────────

class ClosureLiteralDemo
{
    public function demo(): void
    {
        // The parameters an arrow function declares are part of the type it
        // produces, so it satisfies a declared `Closure(Scaffolding\Pen): string`.
        $describe = fn (Scaffolding\Pen $pen) => $pen->color();
        $describe;                                 // Closure(Demo\Pen): string

        (new Scaffolding\ScaffoldingPenDescriber())->describeWith($describe);
    }
}


// ── Conditional Return Types ────────────────────────────────────────────────

class ConditionalReturnDemo
{
    /** A flag reached through a constant is still the same flag. */
    public const JSON_FLAGS = JSON_THROW_ON_ERROR;

    /** And so is one built out of several. */
    public const JSON_COMBO = JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR;

    /** A declared type says what the constant may hold, not what it holds. */
    public const int JSON_TYPED = JSON_HEX_TAG | JSON_THROW_ON_ERROR;

    public function demo(): void
    {
        $container = new Scaffolding\Container();
        $resolved = $container->make(Scaffolding\Pen::class);
        $resolved->write();                       // class-string<T> → T

        $appPen = Scaffolding\app(Scaffolding\Pen::class);                // conditional on standalone function
        $appPen->write();

        // Literal string conditional return type
        $mapper = new Scaffolding\TreeMapperImpl();
        $result = $mapper->map('foo', 'bar');
        $result->write();                         // "foo" → Scaffolding\Pen (literal string match)

        // Out-of-order named arguments bind to the parameter they name, so
        // the conditional keys on `$signature` ("foo") regardless of the
        // order the arguments appear in the call.
        $named = $mapper->map(source: 'bar', signature: 'foo');
        $named->write();                          // named "foo" → Scaffolding\Pen

        // A conditional branch selecting `mixed` flows `mixed` through as a
        // real type, so the value stays usable (and narrowable) instead of
        // becoming untyped.
        $unknown = Scaffolding\sessionValue('file');          // ($key is string ? mixed : null) → mixed
        if (is_string($unknown)) {
            strtoupper($unknown);                 // narrowed from mixed to string
        }

        // A conditional keyed on an argument's *type* is decided by what the
        // call was handed: one id finds one pen, a list of ids finds the
        // whole drawer, and an argument the engine cannot pin down keeps both
        // branches rather than committing to one the call may not take.
        $drawer = new Scaffolding\PenDrawer();
        $onePen = $drawer->find(7);
        $onePen?->write();                        // int id → Scaffolding\Pen|null
        $manyPens = $drawer->find([7, 8]);
        $manyPens->first()->write();              // list of ids → Scaffolding\TypedCollection<Scaffolding\Pen>

        // `is null` follows the argument's type too, so a value that may be
        // null keeps the branch it would take when it is.
        $labelled = $drawer->label('lid');
        strtoupper($labelled);                    // a string label → string
        $unlabelled = $drawer->label();
        strtoupper($unlabelled['lid']);           // no label → array<string, string>

        // A conditional keyed on an argument's *value* reads the literal at
        // the call site, and the parameter's declared default when the
        // argument is left out, instead of every branch at once.
        $wordCount = str_word_count('two words'); // $format defaults to 0 → int
        echo $wordCount + 1;
        $wordList = str_word_count('two words', 1);
        strtoupper($wordList[0]);                 // format 1 → list<string>

        // A `never` branch is an assertion in disguise: the falsy half of
        // the argument cannot have survived the call, so it is subtracted
        // from what follows without an `if` or a `@phpstan-assert` tag.
        $maybePen = Scaffolding\pickPenOrNull();
        Scaffolding\throwUnless($maybePen, 'no pen');
        $maybePen->write();                       // narrowed to Scaffolding\Pen

        // The replace family returns the shape it was handed, so a string
        // subject rules out the array branch its signature also names.
        $swapped = str_replace('a', 'b', 'banana');
        strtoupper($swapped);                     // string subject → string
        $swappedAll = str_replace('a', 'b', ['banana', 'apple']);
        strtoupper($swappedAll[0]);               // array subject → array<array-key, string>

        // The subject is read the same way spelled other ways too: an
        // array-shape element, a global constant, and an elvis expression
        // all rule out the array branch instead of leaving it undecided.
        /** @var array{message: string} $shapeData */
        $shapeData = ['message' => 'banana'];
        $fromShape = str_replace('a', 'b', $shapeData['message']);
        strtoupper($fromShape);                   // array-shape element subject → string
        $fromConst = str_replace('.', '-', PHP_VERSION);
        strtoupper($fromConst);                    // global constant subject → string
        /** @var ?string $maybeBody */
        $maybeBody = null;
        $fromElvis = str_replace('a', 'b', $maybeBody ?: 'banana');
        strtoupper($fromElvis);                    // elvis-operator subject → string

        // `preg_replace()` keeps its `null` error branch for a string
        // subject, where PCRE really can fail, and drops it for an array.
        $masked = preg_replace('/\d/', '*', 'a1b2') ?? '';
        strtoupper($masked);                      // string subject → string|null
        $maskedAll = preg_replace('/\d/', '*', ['a1', 'b2']);
        strtoupper($maskedAll[0]);                // array subject → array<array-key, string>

        // A literal pattern says which keys `preg_match()` fills in: the
        // whole match under `0`, every capture group under its number, and
        // a named group under its name as well.
        if (preg_match('/(?<amount>\d+)(?<unit>\w*)/', '12kg', $size)) {
            strtoupper($size['unit']);            // named group → string
            strtoupper($size[1]);                 // the amount by number → string
            strtoupper($size[0]);                 // the whole match → string
        }

        // Nothing guards this call, and a failed match leaves the empty
        // array behind, so the same read may find no such key.
        preg_match('/(\d+)/', '12kg', $maybe);
        strtoupper($maybe[1] ?? '');              // may be missing → ?string

        // Storing the result first changes nothing: the call still filled
        // the array in, and the variable holding the outcome stands for the
        // match, so testing it later narrows the array the same way.
        $matched = preg_match('/(?<port>\d+)/', 'host:8080', $address);
        if ($matched) {
            strtoupper($address['port']);         // guarded by the result → string
        }

        // `preg_match_all()` collects every match of a group, so the same
        // key holds a list of them rather than one.
        preg_match_all('/(\d+)/', '1, 2, 3', $numbers);
        strtoupper($numbers[1][0]);               // group 1 → list<string>

        // `JSON_THROW_ON_ERROR` raises a JsonException instead of returning
        // `false`, so the failure branch cannot happen at this call site.
        $json = json_encode(['ok' => true], JSON_THROW_ON_ERROR);
        strtoupper($json);                        // string, not string|false

        // The flag counts wherever it is spelled: a constant that holds it,
        // a constant that ORs it with another, and a mask kept in a variable
        // all fold to the value the bit is tested against.
        $aliased = json_encode(['ok' => true], self::JSON_FLAGS);
        strtoupper($aliased);                     // constant alias → string
        $combined = json_encode(['ok' => true], self::JSON_COMBO);
        strtoupper($combined);                    // constant of a mask → string
        $mask = JSON_UNESCAPED_SLASHES | self::JSON_FLAGS;
        $fromMask = json_encode(['ok' => true], $mask);
        strtoupper($fromMask);                    // mask in a variable → string
        $typed = json_encode(['ok' => true], self::JSON_TYPED);
        strtoupper($typed);                       // typed constant → string

        // More builtins whose shape an argument decides. Only the
        // all-elements form of `pathinfo()` returns the component array;
        // every other flag asks for one part and gets a string back.
        $parts = pathinfo('/tmp/report.csv');     // no flag → the component array
        strtoupper($parts['basename']);
        $stem = pathinfo('/tmp/report.csv', PATHINFO_FILENAME);
        strtoupper($stem);                        // one component → string

        // `print_r()` renders to a string only when asked to; otherwise it
        // prints and reports that it did.
        $rendered = print_r(['a' => 1], true);
        strtoupper($rendered);                    // captured → string

        // `microtime()` and `hrtime()` both switch on a boolean flag.
        $seconds = microtime(true);
        echo $seconds + 1.0;                      // as-float → float
        $stamp = microtime();
        strtoupper($stamp);                       // default → string

        // Naming an environment variable returns its value; only the
        // no-argument form returns the whole environment.
        $home = getenv('HOME') ?: '/root';
        strtoupper($home);                        // named → string|false

        // `abs()` returns the type it was given.
        $magnitude = abs(-7);
        echo $magnitude << 1;                     // int argument → int, not int|float

        // `var_export()` renders to a string only for the `$return = true`
        // form; the default prints and hands back nothing at all.
        $exported = var_export(['a' => 1], true);
        strtoupper($exported);                    // captured → string

        // One name, two functions: without an argument each of these reads,
        // with one it writes and reports whether the write took.
        $encoding = mb_internal_encoding();
        strtoupper($encoding);                    // getter form → string
        $ordering = version_compare(PHP_VERSION, '8.2.0');
        echo $ordering << 1;                      // no operator → int, not int|bool

        // The `scanf` family collects into an array, or fills by-reference
        // targets and reports how many it filled.
        $collected = sscanf('12 apples', '%d %s');
        echo count($collected ?? []);             // no targets → array|null
        $filled = sscanf('12 apples', '%d %s', $count, $fruit);
        echo $filled << 1;                        // targets passed → int

        // A `range()` of integers stays integral; one fractional bound makes
        // every element a float.
        $steps = range(0, 10, 2);
        echo $steps[0] << 1;                      // all-int bounds → list<int>
        $fractions = range(0, 1, 0.25);
        echo $fractions[0] + 0.5;                 // fractional step → list<float>

        // `array_reduce()` only answers null for the one case that produces
        // it: an empty array with no initial value.
        $total = array_reduce([1, 2, 3], static fn (int $carry, int $n): int => $carry + $n, 0);
        echo $total << 1;                         // seeded → int, never null

        // `pow()`'s `object` branch belongs to the operator-overloading
        // extensions; two numbers can only produce a number.
        $raised = pow(2, 10);
        echo $raised + 1;                         // numeric operands → int|float

        // `ini_get()` reports false for a directive that is not set, which
        // the core ones always are.
        $limit = ini_get('memory_limit');
        strtoupper($limit);                       // core directive → string

        // `get_class()` names the class of the object it was handed, so the
        // result can be instantiated or passed on as a class-string.
        $pen = new Scaffolding\Pen();
        $penClass = get_class($pen);
        (new $penClass())->write();               // class-string<Scaffolding\Pen>
    }
}


// ── Method-Level @template ──────────────────────────────────────────────────

class MethodTemplateDemo
{
    public function demo(): void
    {
        $locator = new Scaffolding\ServiceLocator();
        $locator->get(Scaffolding\Pen::class)->write();               // class-string<T> → T

        // A class-string passed as a single-quoted string literal: the
        // source `\\` is a namespace-separator escape, so `'Demo\\Scaffolding\\Pen'`
        // names the class `Demo\Scaffolding\Pen` and T resolves to Scaffolding\Pen.
        $locator->get('Demo\\Scaffolding\\Pen')->write(); // string-literal class-string → Scaffolding\Pen

        // A union of class-strings (iterating a class-constant array) binds
        // the bounded template `T of Scaffolding\Pen` to the union of the concrete
        // classes, so `@return T[]` resolves to (Scaffolding\Pen|Scaffolding\Marker)[] instead of
        // collapsing to the bound Scaffolding\Pen[]. Scaffolding\Marker extends Scaffolding\Pen, so both satisfy
        // the bound.
        foreach ([Scaffolding\Pen::class, Scaffolding\Marker::class] as $penClass) {
            $group = $locator->getAll($penClass);
            $group[0]->write();                           // Scaffolding\Pen|Scaffolding\Marker from union class-string bind
        }

        // Indexing the call result inline keeps the template binding, so
        // the `@return T[]` element resolves from the `class-string<T>`
        // argument without an intermediate variable.
        $locator->getAll(Scaffolding\Pen::class)[0]->write();         // Scaffolding\Pen from class-string<T> → T[] element

        Scaffolding\Factory::create(Scaffolding\Pen::class)->write();             // static @template
        Scaffolding\resolve(Scaffolding\Marker::class)->highlight();              // function @template

        // A static factory's method-level @template binds the element
        // type of the collection it returns, whether the result goes
        // through a variable or the call is chained straight through.
        $iteration = new Scaffolding\ScaffoldingIteration();
        $made = Scaffolding\TypedCollection::make($iteration->allPens());
        $made->first()->write();                          // TValue = Scaffolding\Pen via a variable
        Scaffolding\TypedCollection::make($iteration->allPens())->first()->write(); // ...and chained directly

        $mapper = new Scaffolding\ObjectMapper();
        $mapped = $mapper->wrap(new Scaffolding\Pen());
        $mapped->first();                         // → Scaffolding\Pen (T resolved from argument)

        $mapper->wrap(new Scaffolding\Product())->first()->getPrice(); // new expression arg → Scaffolding\Product

        // Untyped class-constant argument binds T to the constant's value
        // type (int), not the constant's owning class.
        $mapper->identity(ConstantTypeDemo::TIMEOUT); // hover → int

        // A `::class` argument binds `@param T` to the argument's actual
        // type — a class-string — so the return type is class-string<Scaffolding\Pen>,
        // not the bare Scaffolding\Pen instance type.
        $mapper->identity(Scaffolding\Pen::class); // hover → class-string<Scaffolding\Pen>

        // Chained instantiation preserves constructor-inferred generics
        (new Scaffolding\ObjectMapper())->wrap(new Scaffolding\Pen())->first()->write(); // (new ...)->method() chain with generics

        // Variadic class-string<T> → union return type
        $locator2 = new Scaffolding\ServiceLocator();
        $union = $locator2->getAny(Scaffolding\Pen::class, Scaffolding\Marker::class);
        $union->write();                                  // A|B from variadic class-string<T>
        $union->highlight();

        // Nested generic return: @return Scaffolding\Box<T> with class-string<T> param
        $boxed = $locator->wrap(Scaffolding\Pen::class);
        $boxed->unwrap()->write();                        // Scaffolding\Box<T>::unwrap() → Scaffolding\Pen

        // A `class-string<T>|T` union parameter (the Mockery::mock()
        // shape, here nested in a variadic array hint) binds T to the
        // named class itself, so a `::class` argument and an instance
        // argument both resolve to a Scaffolding\Pen instance.
        $locator->build(Scaffolding\Pen::class)->write();             // class-string<T>|T with Scaffolding\Pen::class → Scaffolding\Pen
        $locator->build(new Scaffolding\Pen())->write();              // class-string<T>|T with instance → Scaffolding\Pen

        // An identity generic whose *constraint* is an array type: T is
        // never bound from an argument here, only from its declared bound.
        $mapper->peekLast([new Scaffolding\Pen()]);
    }
}


// ── Closure Return Type Template Binding ────────────────────────────────────

class ClosureReturnTemplateDemo
{
    public function demo(): void
    {
        // Method-level @template bound from closure return type annotation.
        // reduce()'s TReduceReturnType is inferred from the closure's `: Scaffolding\Pen` return type.
        /** @var Scaffolding\ScaffoldingReducible<Scaffolding\Pencil> $pencils */
        $pencils = new Scaffolding\ScaffoldingReducible();

        $merged = $pencils->reduce(
            fn(Scaffolding\Pen $carry, Scaffolding\Pencil $item): Scaffolding\Pen => $carry,
            new Scaffolding\Pen('starter')
        );
        $merged->write();       // TReduceReturnType = Scaffolding\Pen

        // Same with function() keyword closure
        $merged2 = $pencils->reduce(
            function(Scaffolding\Pen $carry, Scaffolding\Pencil $item): Scaffolding\Pen {
                return $carry;
            },
            new Scaffolding\Pen('starter')
        );
        $merged2->color();      // TReduceReturnType = Scaffolding\Pen

        // Chained call: reduce() result used directly without intermediate variable.
        // The template inference must survive the symbol-map subject text serialization.
        $pencils->reduce(fn(Scaffolding\Pen $carry, Scaffolding\Pencil $item): Scaffolding\Pen => $carry, new Scaffolding\Pen('starter'))->write();

        // Closure with NO return-type annotation: the return type is
        // inferred from the body expression, resolving `$carry` from the
        // closure's own typed parameter (`$carry->rename(...)` → Scaffolding\Pen).
        $inferred = $pencils->reduce(
            fn(Scaffolding\Pen $carry, $item) => $carry->rename('merged'),
            new Scaffolding\Pen('starter')
        );
        $inferred->write();     // TReduceReturnType = Scaffolding\Pen (from the body)

        // @template T bound from a `@param \Closure(): T` callback, where the
        // closure has NO return-type annotation. The return type is inferred
        // from the closure body (like Laravel's Cache::remember).
        $cache = new Scaffolding\ScaffoldingClosureCache();
        $cache->remember('pen', fn() => new Scaffolding\Pen('cached'))->write();   // arrow body → Scaffolding\Pen
        $cache->remember('marker', function () {
            return new Scaffolding\Marker('cached');
        })->highlight();                                                // block-closure return → Scaffolding\Marker

        // The `static` modifier is just a modifier: a static closure binds
        // the template exactly like a plain one.
        $cache->remember('static-pen', static fn(): Scaffolding\Pen => new Scaffolding\Pen('cached'))->write();
        $cache->remember('static-marker', static function () {
            return new Scaffolding\Marker('cached');
        })->highlight();                                                // static block-closure → Scaffolding\Marker

        // A body that is a *call* binds the template from the call's return
        // type, the same way a `new` expression or a literal body does.
        $cache->remember('made', fn() => Scaffolding\Pen::make('blue'))->write();   // static call → Scaffolding\Pen
        $marker = new Scaffolding\Marker('yellow');
        $cache->remember('renamed', fn() => $marker->rename('bold'))
            ->highlight();                                              // method call → Scaffolding\Marker
    }
}

// ── Closure Param → Template Inference (Contravariant) ─────────────────────

class ClosureParamTemplateDemo
{
    public function demo(): void
    {
        // When a method declares @param Closure(T): void $cb, the template
        // param T is inferred from the closure's *parameter* type annotation
        // (contravariant position), not the return type.

        $bus = new Scaffolding\ScaffoldingEventBus();

        // Arrow function: T inferred as Scaffolding\Pen from fn(Scaffolding\Pen $p)
        $result = $bus->listen(function(Scaffolding\Pen $p): void { $p->write(); });
        $result->write();       // T = Scaffolding\Pen
        $result->color();       // completions for Scaffolding\Pen

        // Full closure: T inferred as Scaffolding\User from function(Scaffolding\User $u)
        $user = $bus->listen(function(Scaffolding\User $u): void { $u->getEmail(); });
        $user->getName();       // T = Scaffolding\User

        // Second param position: @param Closure(int, T): void
        $proc = new Scaffolding\ScaffoldingBatchProcessor();
        $item = $proc->process(function(int $i, Scaffolding\Pencil $p): void { $p->sketch(); });
        $item->sketch();        // T = Scaffolding\Pencil (from position 1)
        $item->sharpen();
    }
}


// ── Template Bound From Several Parameters ──────────────────────────────────

class MultiBoundTemplateDemo
{
    public function demo(): void
    {
        $box = new Scaffolding\ScaffoldingToolbox();

        // Both arguments bind the same `@template T`, so `T` is what the two
        // of them have in common.  Neither is measured against the other:
        // handing one pens and the other pencils reports nothing.
        $mixed = $box->combine([new Scaffolding\Pen('red')], [new Scaffolding\Pencil()]);
        foreach ($mixed as $tool) {
            $tool->label();     // Try: `$tool->` — T = Scaffolding\Pen|Scaffolding\Pencil, so label() only
        }

        // Two arguments holding the same thing keep `T` narrow.
        $pens = $box->combine([new Scaffolding\Pen('red')], [new Scaffolding\Pen('blue')]);
        foreach ($pens as $pen) {
            $pen->write();      // T = Scaffolding\Pen
        }

        // An empty literal has no elements to contribute, so the other
        // argument alone settles `T`.
        $onlySecond = $box->combine([], [new Scaffolding\Pen('blue')]);
        foreach ($onlySecond as $pen) {
            $pen->color();      // T = Scaffolding\Pen
        }
    }
}


// ── Receiver Retyped By A Call (@psalm-this-out / @phpstan-self-out) ────────

class SelfOutDemo
{
    public function demo(): void
    {
        /** @var Scaffolding\ScaffoldingMutableBox<Scaffolding\Pen> $box */
        $box = new Scaffolding\ScaffoldingMutableBox(new Scaffolding\Pen('red'));
        $box->value->write();       // Try: `$box->value->` — T = Scaffolding\Pen

        // `replace()` carries `@psalm-this-out self<U>`, so the call rebinds
        // $box's own template argument for the rest of the block the way an
        // assignment would.  U is bound from the argument, so T becomes
        // Scaffolding\Pencil and the Scaffolding\Pen members are gone.
        $box->replace(new Scaffolding\Pencil());
        $box->value->sketch();      // Try: `$box->value->` — T = Scaffolding\Pencil now
    }
}


// ── Trait Generic Substitution ──────────────────────────────────────────────

class TraitGenericDemo
{
    public function demo(): void
    {
        Scaffolding\Product::factory()->create();             // @use Scaffolding\HasFactory<Scaffolding\UserFactory> → Scaffolding\UserFactory
        Scaffolding\Product::factory()->count(5)->make();     // count() returns static, make() returns Scaffolding\Product

        $idx = new Scaffolding\PenIndex();                    // @use Scaffolding\Indexable<int, Scaffolding\Pen>
        $idx->get()->write();                     // TValue → Scaffolding\Pen
    }
}


// ── Null-Init + Conditional Reassignment ────────────────────────────────────

class NullInitReassignDemo
{
    /** @param list<Scaffolding\Pen> $pens */
    public function demo(array $pens): void
    {
        // Pattern 1: null-init + foreach reassignment + truthiness guard
        $found = null;
        foreach ($pens as $pen) {
            if ($pen->color() === 'blue') {
                $found = $pen;
            }
        }
        if ($found) {
            $found->write();                      // Scaffolding\Pen from foreach reassignment
        }

        // Pattern 2: null-coalesce + guard inside foreach
        /** @var array<string, Scaffolding\Pen> $lookup */
        $lookup = Scaffolding\getUnknownValue();
        $keys = ['a', 'b'];
        foreach ($keys as $key) {
            $item = $lookup[$key] ?? null;
            if (!$item) { continue; }
            $item->write();                       // Scaffolding\Pen from array access via coalesce
        }
    }
}


// ── Loop-Carried Assignment ─────────────────────────────────────────────────
// When a variable is initialized as null and reassigned inside a loop body,
// the assignment from a previous iteration is visible at the top of the loop.

class LoopCarriedAssignmentDemo
{
    /** @param list<Scaffolding\Pen> $pens */
    public function demo(array $pens): void
    {
        // Pattern: null-init + reassignment after the usage point in the loop.
        // On the second iteration, $prev holds the Scaffolding\Pen from the prior iteration.
        $prev = null;
        foreach ($pens as $pen) {
            if ($prev !== null) {
                $prev->write();                   // Scaffolding\Pen from previous iteration
            }
            $prev = $pen;
        }

        // Same pattern with a while loop
        $lastOrder = null;
        while ($row = rand(0, 1)) {
            if ($lastOrder !== null) {
                $lastOrder->getStatusCode();      // Scaffolding\Response from previous iteration
            }
            $lastOrder = new Scaffolding\Response(200, 'ok');
        }
    }

    /**
     * A read loop advances its own cursor, so the checked variable is
     * written to below the read. The read still sees what the loop
     * condition established.
     *
     * @param list<string> $lines
     */
    public function readLoop(array $lines): string
    {
        $joined = '';
        $line = Scaffolding\demoNextLine($lines);
        while ($line !== false) {
            $joined .= strtoupper($line);         // string (`!== false` holds here)
            $line = Scaffolding\demoNextLine($lines);         // string|false again, below the read
        }

        return $joined;
    }

    /**
     * The compact form of the same loop reads and checks in one
     * expression, so the assignment is the subject the check narrows.
     *
     * @param list<string> $lines
     */
    public function compactReadLoop(array $lines): string
    {
        $joined = '';
        while (($line = Scaffolding\demoNextLine($lines)) !== false) {
            $joined .= strtoupper($line);         // string (`false` narrowed away)
        }

        // The bare truthy form rules out `false` the same way.
        $trailing = ['omega'];
        while ($next = Scaffolding\demoNextLine($trailing)) {
            $joined .= strtolower($next);         // string
        }

        return $joined;
    }

    /**
     * The fold-into-an-accumulator loop: seed on the pass where the
     * variable is still `null`, merge into it on every pass after. On the
     * first pass the merge branch cannot be entered at all, so the
     * impossible `null->mergeWith()` it would perform is not an
     * alternative to what the seed branch produced.
     *
     * @param list<Scaffolding\DrawingStep> $steps
     */
    public function foldAccumulator(array $steps): int
    {
        $tally = null;
        foreach ($steps as $step) {
            if ($tally === null) {
                $tally = $step->tally();
                continue;
            }
            $tally = $tally->mergeWith($step->tally());   // Scaffolding\InkTally
        }

        // Try: put the cursor after the `?->` below. The loop may not have
        // run at all, so `$tally` is `Scaffolding\InkTally|null` here.
        // $tally?->

        return $tally?->total() ?? 0;
    }

    /**
     * The same fold written as an `if`/`else`, and as the ternary the
     * idiom is usually compressed into. Both branches of the check are
     * the same two the guard-clause form above spells out.
     *
     * @param list<Scaffolding\DrawingStep> $steps
     */
    public function foldAccumulatorTernary(array $steps): int
    {
        $viaElse = null;
        foreach ($steps as $step) {
            if ($viaElse === null) {
                $viaElse = $step->tally();
            } else {
                $viaElse = $viaElse->mergeWith($step->tally());   // Scaffolding\InkTally
            }
        }

        $viaTernary = null;
        foreach ($steps as $step) {
            $viaTernary = $viaTernary === null
                ? $step->tally()
                : $viaTernary->mergeWith($step->tally());         // Scaffolding\InkTally
        }

        return ($viaElse?->total() ?? 0) + ($viaTernary?->total() ?? 0);
    }
}


// ── Loops Over an Array That Cannot Be Empty ────────────────────────────────
// A `foreach` over an array proven to hold entries runs its body at least
// once, so the sentinel the loop was seeded with is gone by the time the
// loop ends and a variable first written inside it is defined after it.

class NonEmptyLoopDemo
{
    /**
     * Try: put the cursor after `$smallest->` and trigger completion.
     *
     * The `!$pens` guard proves the list non-empty, so `$smallest` is a
     * `Scaffolding\Pen` here rather than `Scaffolding\Pen|null`.
     *
     * @param list<Scaffolding\Pen> $pens
     */
    public function smallest(array $pens): Scaffolding\Pen
    {
        if (!$pens) {
            return new Scaffolding\Pen();
        }
        $smallest = null;
        foreach ($pens as $pen) {
            if ($smallest === null) {
                $smallest = $pen;
                continue;
            }
            $smallest = strlen($pen->color()) < strlen($smallest->color()) ? $pen : $smallest;
        }
        $smallest->write();                       // Scaffolding\Pen (the pre-loop null is gone)

        return $smallest;
    }

    /**
     * The same proof written into the annotation instead of a guard. A
     * `non-empty-array`, a shape with a required key, and a literal
     * written with entries all say the loop body runs.
     *
     * @param non-empty-list<Scaffolding\Pen> $pens
     */
    public function labels(array $pens): string
    {
        $last = null;
        foreach ($pens as $pen) {
            $last = $pen;
        }
        $summary = $last->color();                // string (never null here)

        foreach (['red', 'blue'] as $name) {
            $seen = $name;
        }

        return $summary . ' ' . $seen;            // string, defined by the loop
    }

    /**
     * Nothing proves this array holds entries, so the loop may not run
     * and the pre-loop `null` survives it.
     *
     * @param list<Scaffolding\Pen> $pens
     */
    public function maybeLast(array $pens): ?Scaffolding\Pen
    {
        $last = null;
        foreach ($pens as $pen) {
            $last = $pen;
        }

        return $last;                             // Scaffolding\Pen|null
    }
}


// ── Pre-Validation Loops ────────────────────────────────────────────────────
// A loop that rejects the whole collection the moment one entry fails a
// check only falls out of its own bottom once every entry has passed, so
// the code after it may treat the collection as holding the checked type.
// Whether the collection was empty does not matter: the claim is vacuously
// true when the body never ran.

class PreValidationLoopDemo
{
    /**
     * Validate every node up front, then walk the list again to use them.
     * The second loop reads members the first loop proved are there
     * without checking a second time.
     *
     * @param list<Scaffolding\SketchGroup> $groups
     */
    public function captions(array $groups): string
    {
        $out = '';
        foreach ($groups as $group) {
            foreach ($group->nodes as $node) {
                // `break 2` skips the second loop entirely, so reaching it
                // means every node passed.
                if (!$node instanceof Scaffolding\LabelledSketchNode) {
                    break 2;
                }
            }

            foreach ($group->nodes as $checked) {
                $out .= $checked->caption();      // Scaffolding\LabelledSketchNode
            }
        }

        return $out;
    }

    /** A `return` guard proves it for the rest of the function the same way. */
    public function firstCaption(Scaffolding\SketchGroup $group): string
    {
        foreach ($group->nodes as $node) {
            if (!$node instanceof Scaffolding\LabelledSketchNode) {
                return '';
            }
        }

        foreach ($group->nodes as $checked) {
            return $checked->caption();           // Scaffolding\LabelledSketchNode
        }

        return '';
    }

    /**
     * A plain `break` jumps to exactly the code the claim would be about,
     * and a `continue` skips the entry rather than the rest of the
     * program, so neither proves anything about the collection.
     */
    public function unproven(Scaffolding\SketchGroup $group): string
    {
        $out = '';
        foreach ($group->nodes as $node) {
            if (!$node instanceof Scaffolding\LabelledSketchNode) {
                break;
            }
        }

        // Try: put the cursor after the `->` below. The list is still
        // `Scaffolding\SketchNode`, so only `kind()` is offered.
        foreach ($group->nodes as $unchecked) {
            $out .= $unchecked->kind();           // Scaffolding\SketchNode
        }

        return $out;
    }
}


// ── By-Reference Capture Written on the Way Out ─────────────────────────────
// A callback that gathers what it is looking for into a `use (&$x)` capture
// pushes and returns in the same branch. Returning out of a closure ends it
// just as falling off the bottom does, so what the capture was handed on the
// way out still reaches the caller.

class ByRefCaptureGatherDemo
{
    /** Gather every labelled node, then read the captions off the pile. */
    public function captions(Scaffolding\SketchGroup $group): string
    {
        $labelled = [];
        $group->walk(static function (Scaffolding\SketchNode $node) use (&$labelled): void {
            if ($node instanceof Scaffolding\LabelledSketchNode) {
                $labelled[] = $node;
                return;
            }
        });

        $out = '';
        // Try: put the cursor after the `->` below. The pushed element type
        // outlives the closure's `return`, so `caption()` is offered.
        foreach ($labelled as $node) {
            $out .= $node->caption();             // Scaffolding\LabelledSketchNode
        }

        return $out;
    }

    /** A plain variable written in a guard clause carries the same way. */
    public function firstCaption(Scaffolding\SketchGroup $group): string
    {
        $found = null;
        $group->walk(static function (Scaffolding\SketchNode $node) use (&$found): void {
            if ($found === null && $node instanceof Scaffolding\LabelledSketchNode) {
                $found = $node;
                return;
            }
        });

        // The callback may never have run, so the capture is still nullable.
        return $found?->caption() ?? '';          // Scaffolding\LabelledSketchNode|null
    }
}


// ── Assignment Inside a Condition ───────────────────────────────────────────
// A variable assigned in an `if`/`while` condition is a definition site,
// including the bare negated guard and the call-wrapped form.

class ConditionAssignmentDemo
{
    /** @return ?Scaffolding\Pen */
    public function maybePen(): ?Scaffolding\Pen { return rand(0, 1) ? new Scaffolding\Pen() : null; }

    public function demo(): void
    {
        // Bare negated guard: PHP parses this as `!($pen = $this->maybePen())`.
        if (!$pen = $this->maybePen()) {
            return;
        }
        $pen->write();                            // Scaffolding\Pen (assignment seen through `!`)

        // Assignment wrapped in a call argument.
        while (is_object($next = $this->maybePen())) {
            $next->write();                       // Scaffolding\Pen (assignment seen inside is_object())
        }
    }

    /** The sentinel check narrows what the assignment beside it produced. */
    public function firstPen(): ?Scaffolding\Pen
    {
        if (($pens = Scaffolding\loadPensOrFail()) !== false) {
            return $pens[0];                      // Scaffolding\Pen (`false` narrowed away)
        }

        return null;
    }
}


// ── Null Coalesce (`??`) Refinement ─────────────────────────────────────────

class NullCoalesceDemo
{
    /** @return ?Scaffolding\Pen */
    public function maybePen(): ?Scaffolding\Pen { return rand(0, 1) ? new Scaffolding\Pen() : null; }

    public function demo(): void
    {
        // Non-nullable LHS: `new Foo()` can never be null, so the RHS
        // is dead code and the result resolves to Scaffolding\Pen only.
        $a = new Scaffolding\Pen() ?? new Scaffolding\Marker();
        $a->write();                              // Scaffolding\Pen (RHS ignored)

        // Nullable LHS: `?Scaffolding\Pen` return strips null, unions with RHS.
        $b = $this->maybePen() ?? new Scaffolding\Marker();
        $b->write();                              // Scaffolding\Pen|Scaffolding\Marker

        // Clone is non-nullable — RHS is dead code.
        $pen = new Scaffolding\Pen();
        $c = clone $pen ?? new Scaffolding\Marker();
        $c->write();                              // Scaffolding\Pen (RHS ignored)
    }
}


// ── Foreach & Array Access ──────────────────────────────────────────────────

class ForeachArrayAccessDemo
{
    public function demo(): void
    {
        /** @var list<Scaffolding\Pen> $members */
        $members = Scaffolding\getUnknownValue();
        foreach ($members as $member) {
            $member->write();                     // element type from list<Scaffolding\Pen>
        }
        $members[0]->color();                     // array access element type

        /** @var array<int, Scaffolding\Pen> */
        $annotated = [];                          // @var without variable name
        $annotated[0]->write();                   // type from next-line annotation

        $inferred = [new Scaffolding\Pen(), new Scaffolding\Marker()];
        $inferred[0]->write();                    // element type inferred from literal
    }

    /**
     * An inline `@var` refines a broadly-typed parameter (here `mixed`)
     * before iterating it, so the loop variable resolves to the element
     * type even though the parameter itself carries no useful type.
     */
    public function demoRetypedParam(mixed $pens): void
    {
        /** @var iterable<Scaffolding\Pen> $pens */
        foreach ($pens as $pen) {
            $pen->write();                        // Scaffolding\Pen from the inline @var retype
        }
    }

    /**
     * The same inline `@var` retype works when the iterable is a method
     * chain: the annotation types the base variable, the chain resolves
     * through it, and the loop variable gets the element type.
     */
    public function demoRetypedChainBase(mixed $holder): void
    {
        /** @var Scaffolding\ScaffoldingConcreteHolder $holder */
        foreach ($holder->getPens() as $pen) {
            $pen->write();                        // Scaffolding\Pen via the @var-typed chain base
        }
    }
}

// ── Foreach By-Reference ────────────────────────────────────────────────────

class ForeachByReferenceDemo
{
    public function demo(): void
    {
        /** @var list<Scaffolding\Pen> $pens */
        $pens = Scaffolding\getUnknownValue();

        // By-reference foreach: $pen resolves to element type (Scaffolding\Pen)
        // and is not flagged as undefined or unused.
        foreach ($pens as &$pen) {
            $pen->write();                        // Scaffolding\Pen from list<Scaffolding\Pen>
            $pen = new Scaffolding\Pen();                     // reassignment through reference
        }
        unset($pen);

        // Key-value with by-reference value
        /** @var array<string, Scaffolding\Pen> $named */
        $named = Scaffolding\getUnknownValue();
        foreach ($named as $key => &$tool) {
            $tool->color();                       // Scaffolding\Pen from array<string, Scaffolding\Pen>
        }
        unset($tool);
    }
}


// ── Property Array Access (generic annotations) ────────────────────────────

class PropertyArrayAccessDemo
{
    /** @var array<string, Scaffolding\Pen> */
    private array $cache = [];

    /** @var array<int, Scaffolding\Pen> */
    public array $items = [];

    public function demo(): void
    {
        // Property typed as array<string, Scaffolding\Pen> — variable key
        $this->cache[$this->getKey()]->write();   // element type from generic annotation

        // Property typed as array<string, Scaffolding\Pen> — string-literal key
        $this->cache['brushes']->color();         // element type from generic annotation

        // Property typed as array<int, Scaffolding\Pen> — numeric index
        $this->items[0]->write();                 // element type from generic annotation

        // Method chain after bracket access
        $this->cache['tools']->rename('Fine')->write(); // chain through element type
    }

    private function getKey(): string { return 'k'; }
}


// ── Array Destructuring ────────────────────────────────────────────────────

class ArrayDestructuringDemo
{
    public function demo(): void
    {
        /** @var list<Scaffolding\Pen> */
        [$first, $second] = Scaffolding\getUnknownValue();
        $first->write();                          // destructured element type
    }
}


// ── Array Shapes ────────────────────────────────────────────────────────────

class ArrayShapeDemo
{
    public function demo(): void
    {
        // Literal array shape — key completion and value types
        $config = ['host' => 'localhost', 'port' => 3306, 'tool' => new Scaffolding\Pen()];
        $config[''];                              // Try: key completion: host, port, tool
        $config['tool']->write();                 // value type → Scaffolding\Pen

        // Annotated shape
        /** @var array{first: Scaffolding\Pen, second: Scaffolding\Pencil} $pair */
        $pair = Scaffolding\getUnknownValue();
        $pair['first']->write();
        $pair['second']->sketch();

        // Shape from function return type
        $cfg = Scaffolding\getAppConfig();
        $cfg['logger']->write();
    }
}


// ── Object Shapes ───────────────────────────────────────────────────────────

class ObjectShapeDemo
{
    public function demo(): void
    {
        /** @var object{title: string, score: float} $item */
        $item = Scaffolding\getUnknownValue();
        $item->title;                             // Ctrl+Click → jumps to `title:` in docblock above
        $item->score;                             // Ctrl+Click → jumps to `score:` in docblock above
    }
}


// ── Spread Operator Type Tracking ───────────────────────────────────────────

class SpreadOperatorDemo
{
    public function demo(): void
    {
        /** @var list<Scaffolding\Pen> */
        $penList = [];
        /** @var list<Scaffolding\Pencil> */
        $pencilList = [];

        $allPens = [...$penList];
        $allPens[0]->write();                     // resolves Scaffolding\Pen from spread

        $everything = [...$penList, ...$pencilList];
        $everything[0]->label();                  // union: Scaffolding\Pen|Scaffolding\Pencil from multiple spreads
    }
}


// ── Clone Expression ────────────────────────────────────────────────────────

class CloneDemo
{
    public function demo(): void
    {
        $pen = new Scaffolding\Pen('blue');
        $copy = clone $pen;
        $copy->write();                           // preserves Scaffolding\Pen type
    }
}


// ── Class-String Variable Static Access ─────────────────────────────────────

class ClassStringStaticDemo
{
    public function demo(): void
    {
        $cls = Scaffolding\Pen::class;
        $cls::make();                             // static method from Scaffolding\Pen
    }
}


// ── Class-String Parameter Static Dispatch ──────────────────────────────────

class ClassStringParamDispatchDemo
{
    /**
     * @param class-string<\BackedEnum> $enumClass
     */
    public function demo(string $enumClass): void
    {
        // Static method dispatch through class-string<T> parameter.
        // $enumClass::from() returns static, resolved to BackedEnum.
        $result = $enumClass::from('foo');
        $result->name;                            // property from UnitEnum via BackedEnum

        // Foreach over $enumClass::cases() resolves items to BackedEnum.
        foreach ($enumClass::cases() as $item) {
            $item->value;                         // property from BackedEnum
            $item->name;                          // property from UnitEnum
        }
    }
}


// ── Ambiguous Variables ─────────────────────────────────────────────────────

class AmbiguousVariableDemo
{
    public function demo(): void
    {
        if (rand(0, 1)) {
            $ambiguous = new Scaffolding\Lamp();
        } else {
            $ambiguous = new Scaffolding\Faucet();
        }
        $ambiguous->turnOff();                    // available on both branches
        $ambiguous->dim();                        // available on Scaffolding\Lamp branches
        $ambiguous->drip();                       // available on Scaffolding\Faucet branches
    }
}


// ── Parenthesized Assignment ────────────────────────────────────────────────

class ParenthesizedAssignmentDemo
{
    public function demo(): void
    {
        $parenPen = (new Scaffolding\Pen('red'));
        $parenPen->write();                       // resolves through parentheses
    }
}


// ── String Interpolation ────────────────────────────────────────────────────

class StringInterpolationDemo
{
    public function demo(): void
    {
        $pen = new Scaffolding\Pen('blue');
        echo "Ink is {$pen->color()}";             // brace interpolation — full completion
        echo "Tool: $pen->ink";                    // simple interpolation
        echo 'no $pen-> here';                     // single-quoted — suppressed
    }
}


// ── Foreach over Generic Collection Classes ─────────────────────────────────

class CollectionForeachDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingCollectionForeach();

        // From method return type
        foreach ($src->allPens() as $pen) {
            $pen->write();                // via method return type → collection generics
        }

        // From new instance
        $items = new Scaffolding\PenCollection();
        foreach ($items as $item) {
            $item->color();               // resolves to Scaffolding\Pen via @extends generics
        }

        // From property type
        foreach ($src->pens as $pen) {
            $pen->color();                // via property type → collection generics
        }

        // From variable
        $collection = $src->allPens();
        foreach ($collection as $pen) {
            $pen->write();                // via variable assignment scanning
        }

        // Conditional @return with nested static<...> generic branches
        // (Laravel's Collection::chunk shape). Chunking yields a
        // collection of collections, so each iterated batch is itself a
        // collection whose elements are the original element type.
        /** @var Scaffolding\TypedCollection<int, Scaffolding\Pen> $pens */
        $pens = new Scaffolding\TypedCollection();
        foreach ($pens->chunk(1) as $batch) {
            $batch->first()->color();     // $batch → Scaffolding\TypedCollection<int, Scaffolding\Pen>, first() → Scaffolding\Pen
        }
    }
}


// ── Type Aliases (@phpstan-type / @phpstan-import-type) ─────────────────────

/**
 * @phpstan-type UserData array{name: string, email: string, pen: Scaffolding\Pen}
 * @phpstan-type StatusInfo array{code: int, label: string, owner: Scaffolding\User}
 * @phpstan-type UserList array<int, Profile>
 */
class TypeAliasDemo
{
    public function demo(): void
    {
        $data = $this->getUserData();
        $data['name'];                    // @phpstan-type → array shape key completion
        $data['pen']->write();            // object value → method completion

        $status = $this->getStatus();
        $status['label'];                 // StatusInfo alias → array shape keys
        $status['owner']->getEmail();     // object value → method completion

        // Type alias resolves through foreach iteration
        foreach ($this->getUsers() as $user) {
            $user->getDisplayName();      // UserList → array<int, Profile> → Profile
        }
    }

    /** @return UserData */
    public function getUserData(): array
    {
        return ['name' => 'Alice', 'email' => 'alice@example.com', 'pen' => new Scaffolding\Pen()];
    }

    /** @return StatusInfo */
    public function getStatus(): array
    {
        return ['code' => 200, 'label' => 'OK', 'owner' => new Scaffolding\User('Alice', 'alice@example.com')];
    }

    /** @return UserList */
    public function getUsers(): array
    {
        return [];
    }
}

/**
 * @phpstan-import-type UserData from TypeAliasDemo
 * @phpstan-import-type StatusInfo from TypeAliasDemo as AliasedStatus
 */
class TypeAliasImportDemo
{
    public function demo(): void
    {
        $user = $this->fetchUser();
        $user['email'];                   // imported UserData → array shape keys
        $user['pen']->color();            // object value → method completion

        $status = $this->fetchStatus();
        $status['code'];                  // AliasedStatus → StatusInfo → array shape keys
        $status['owner']->getName();      // object value → method completion
    }

    /** @return UserData */
    public function fetchUser(): array
    {
        return ['name' => 'Bob', 'email' => 'bob@example.com', 'pen' => new Scaffolding\Pen()];
    }

    /** @return AliasedStatus */
    public function fetchStatus(): array
    {
        return ['code' => 404, 'label' => 'Not Found', 'owner' => new Scaffolding\User('Bob', 'bob@example.com')];
    }
}


// ── Multi-line @return & Broken Docblock Recovery ───────────────────────────

class BrokenDocblockDemo
{
    public function demo(): void
    {
        $collection = Scaffolding\collect([]);
        $collection->groupBy('key');             // multi-line @return resolves correctly

        // Nested conditional in the generic return collapses against the
        // argument: grouping by a string key makes the result's key type
        // `array-key`, so passing a string to `get()` type-checks cleanly
        // instead of comparing against a raw, unevaluated conditional.
        $collection->groupBy('key')->get('bucket');

        $recovered = (new Scaffolding\BrokenDocRecovery())->broken();
        $recovered->working();                   // recovers `static` from broken @return static<
    }
}


// ── Callable / Closure Variable Invocation ──────────────────────────────────

class ClosureInvocationDemo
{
    public function demo(): void
    {
        // Closure literal with native return type hint
        $makePen = function(): Scaffolding\Pen { return new Scaffolding\Pen(); };
        $makePen()->write();                      // resolves Scaffolding\Pen from closure return type

        // Arrow function literal
        $makePencil = fn(): Scaffolding\Pencil => new Scaffolding\Pencil();
        $makePencil()->sketch();                  // arrow fn return type

        // Docblock callable annotation
        /** @var \Closure(): Scaffolding\Pencil $supplier */
        $supplier = Scaffolding\getUnknownValue();
        $supplier()->sharpen();                   // @var Closure() annotation

        // Chaining after callable invocation
        $builder = function(): Scaffolding\Pen { return new Scaffolding\Pen(); };
        $builder()->rename('Bold')->write();      // chain after $fn()

        // Variable assigned from callable invocation
        $fromClosure = $makePen();
        $fromClosure->write();                    // $result = $fn() resolves return type

        // Immediately invoked arrow function with return type
        $result = (fn(): Scaffolding\Pen => new Scaffolding\Pen())();
        $result->write();                         // resolves Scaffolding\Pen from arrow fn return type

        // Immediately invoked closure with return type
        $obj = (function(): Scaffolding\Pencil { return new Scaffolding\Pencil(); })();
        $obj->sketch();                           // resolves Scaffolding\Pencil from closure return type
    }
}


// ── class-string Variable Resolution ────────────────────────────────────────

class ClassStringVarDemo
{
    public function demo(): void
    {
        // new $var where $var holds a class-string
        $cls = Scaffolding\Pen::class;
        $pen = new $cls;
        $pen->write();                            // resolves Scaffolding\Pen from class-string

        // $var::staticMethod() where $var holds a class-string
        $userClass = Scaffolding\User::class;
        $found = $userClass::findByEmail('test@example.com');
        $found->getEmail();                       // resolves Scaffolding\User from class-string static call
    }

    /**
     * A `class-string<Scaffolding\Pen>` variable that passes through a
     * `class_exists()` guard keeps its `<Scaffolding\Pen>` type argument, so
     * `new $className()` still resolves to `Scaffolding\Pen`.
     */
    public function guardedInstantiation(mixed $rawName): object
    {
        /** @var class-string<Scaffolding\Pen> */
        $className = (string) $rawName;

        if (!class_exists($className)) {
            throw new \RuntimeException('missing');
        }

        $pen = new $className();
        $pen->write();                            // resolves Scaffolding\Pen despite the class_exists() guard
        return $pen;
    }
}


// ── class-string Property Resolution ────────────────────────────────────────
// A property declared `class-string<T>` holds a class name at runtime, not
// an instance of its own declared type, so a `::` access on it resolves
// against `T`. A property that is merely a plain `string` also holds a
// class name at runtime for all PHPantom can prove, so `::` on it is left
// unresolved rather than reported as scalar access — see
// ScalarMemberAccessDemo in diagnostics.php for the case that is still flagged.

class ClassStringPropertyDemo
{
    /** @var class-string<Scaffolding\Pen> */
    public string $penClass;

    public string $anyClassName;

    public function demo(): void
    {
        $this->penClass::make()->write();  // resolves Scaffolding\Pen through class-string<T> property

        $this->anyClassName::create();     // no diagnostic — unverifiable, not scalar access
    }
}


// ── iterator_to_array Resolution ────────────────────────────────────────────

class IteratorToArrayDemo
{
    public function demo(): void
    {
        /** @var \Iterator<int, Scaffolding\Pen> $iter */
        $iter = Scaffolding\getUnknownValue();

        $items = iterator_to_array($iter);
        $items[0]->write();                       // resolves Scaffolding\Pen from iterator value type

        // The element type survives without the intermediate variable,
        // and through a nested array function call.
        iterator_to_array($iter)[0]->write();     // Scaffolding\Pen straight off the call
        array_values(iterator_to_array($iter))[0]->write();
    }
}


// ── Compound Negated Guard Clause Narrowing ─────────────────────────────────

class CompoundNegatedNarrowingDemo
{
    /** @param Scaffolding\Rock|Scaffolding\Banana|Scaffolding\Lamp $thing */
    public function demo($thing): void
    {
        // After both negated instanceof checks exit, $thing is Scaffolding\Rock|Scaffolding\Banana
        if (!$thing instanceof Scaffolding\Rock && !$thing instanceof Scaffolding\Banana) {
            return;
        }

        $thing->weigh();                          // both Scaffolding\Rock and Scaffolding\Banana have weigh()
    }
}


// ── __invoke() Return Type Resolution ───────────────────────────────────────

class InvokeReturnTypeDemo
{
    public function demo(): void
    {
        // Objects with __invoke() can be called like functions.
        // PHPantom resolves the return type through __invoke().
        $formatter = new Scaffolding\ScaffoldingFormatter();
        $formatter()->write();                    // __invoke() returns Scaffolding\Pen

        // Try: type `$formatter->` — implemented magic methods such as
        // __invoke() and __toString() are offered for explicit calls and
        // go-to-definition, sorted below the regular methods so they never
        // appear at the top of the list.
        $formatter->__invoke()->write();          // explicit __invoke() call

        // Chaining through __invoke() return type
        $factory = new Scaffolding\ScaffoldingPenFactory();
        $factory()->rename('Fine')->write();      // __invoke() → Scaffolding\Pen → rename() → Scaffolding\Pen

        // Parenthesized property invocation: ($this->prop)()
        ($this->formatter)()->write();            // resolves through __invoke()

        // Foreach over __invoke() return type with docblock
        $fetcher = new Scaffolding\ScaffoldingPenFetcher();
        foreach ($fetcher() as $item) {
            $item->write();                       // @return Scaffolding\Pen[] on __invoke()
        }

        // Enum from()/tryFrom() chains to instance methods
        Scaffolding\Status::from('Active')->label();          // from() returns Scaffolding\Status
    }

    private Scaffolding\ScaffoldingFormatter $formatter;
}


// ── Anonymous Classes ───────────────────────────────────────────────────────

class AnonymousClassDemo
{
    public function demo(): object
    {
        return new class extends Scaffolding\Pen {
            public string $brand;
            public function cap(): string { return ''; }
            public function demo() {
                $this->cap();                    // own method
                $this->brand;                    // own property
                $this->write();                  // inherited from Scaffolding\Pen
                // MUST NOT appear: refill() (private — not inherited)
            }
        };
    }
}


// ── Match / Ternary / Null-Coalescing Type Accumulation ─────────────────────

class ExpressionTypeDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingExpressionType();

        // Null-coalescing
        $fallback = $src->backup ?? $src->primary;
        $fallback->getStatusCode();       // Scaffolding\Response method

        // Match expression — shared members sort above branch-only members
        $service = match (rand(0, 1)) {
            0 => new Scaffolding\ElasticProductReviewIndexService(),
            1 => new Scaffolding\ElasticBrandIndexService(),
        };
        $service->index();                // on both — sorted first
        $service->reindex();              // one branch only — sorted after

        // Ternary
        $svc = rand(0, 1)
            ? new Scaffolding\ElasticProductReviewIndexService()
            : new Scaffolding\ElasticBrandIndexService();
        $svc->index();                    // on both — sorted first
    }
}


// ── Switch Statement Type Tracking ──────────────────────────────────────────

class SwitchDemo
{
    public function demo(string $type): void
    {
        switch ($type) {
            case 'reviews':
                $service = new Scaffolding\ElasticProductReviewIndexService();
                break;
            case 'brands':
                $service = new Scaffolding\ElasticBrandIndexService();
                break;
        }
        $service->index();                // on both classes
    }
}


// ── Array & Object Shapes in Methods ────────────────────────────────────────

class ShapeMethodDemo
{
    public function demo(): void
    {
        $data = $this->getToolKit();
        $data['pen']->write();            // Scaffolding\Pen
        $data['pencil']->sketch();        // Scaffolding\Pencil

        // Nested annotated shape
        /** @var array{meta: array{page: int, total: int}, items: list<Scaffolding\Pen>} $response */
        $response = Scaffolding\getUnknownValue();
        $response['meta']['page'];        // nested shape key
        $response['items'][0]->write();   // list element type

        // Nested keys inferred from literal — no annotation needed
        $config = ['db' => ['host' => 'localhost', 'port' => 3306], 'debug' => true];
        $config['db']['host'];            // Try: delete 'host' and trigger completion

        // Object shapes
        $profile = $this->getProfile();
        $profile->name;                   // Ctrl+Click → jumps to `name:` in @return docblock

        $result = $this->getResult();
        $result->tool->write();           // Ctrl+Click `tool` → jumps to `tool:` in @return docblock
        $result->meta->page;              // Ctrl+Click `meta` → jumps to `meta:` in @return docblock
    }

    /** @return array{pen: Scaffolding\Pen, pencil: Scaffolding\Pencil, active: bool} */
    public function getToolKit(): array { return ['pen' => new Scaffolding\Pen(), 'pencil' => new Scaffolding\Pencil(), 'active' => true]; }

    /** @return object{name: string, age: int, active: bool} */
    public function getProfile(): object { return (object) ['name' => 'Ada', 'age' => 36, 'active' => true]; }

    /** @return object{tool: Scaffolding\Pen, meta: object{page: int, total: int}} */
    public function getResult(): object { return (object) ['tool' => new Scaffolding\Pen(), 'meta' => (object) ['page' => 1, 'total' => 3]]; }

    /** @param array{host: string, port: int, tool: Scaffolding\Pen} $config */
    public function fromParam(array $config): void
    {
        $config['host'];                  // string
        $config['tool']->write();         // Scaffolding\Pen
    }

    // Indexing with a dynamic (non-literal) key resolves to the union of
    // the shape's value types.
    public function dynamicKey(string $which): void
    {
        $kit = $this->getToolKit();
        $tool = $kit[$which] ?? null;     // Scaffolding\Pen|Scaffolding\Pencil|bool|null
        if ($tool instanceof Scaffolding\Pen) {
            $tool->write();               // Scaffolding\Pen after instanceof
        }

        // A map of shapes built through a dynamic-key write reads back
        // element-by-element.
        $sums = [];
        foreach ([1, 2] as $id) {
            $sums[$id] = $this->getToolKit();
            $sums[$id]['pen']->write();   // Scaffolding\Pen through the dynamic-key write
        }

        // Nested dynamic writes read back across the same key path.
        $report = [];
        $count = 0;
        foreach ([new Scaffolding\Pen(), new Scaffolding\Pen()] as $pen) {
            $report['data'][$count]['tool'] = $pen;
            $entry = $report['data'][$count]['tool'];
            $entry->write();              // Scaffolding\Pen
            $count++;
        }
    }

    /**
     * An array literal states its contents, so the exact values it names
     * survive a read off it — including a read through a key that is only
     * known at runtime. Mutating the array afterwards is what gives that
     * up: a value written after construction stands in for however many
     * more follow, so the stored values widen to their base types.
     */
    public function literalValuesSurviveTheRead(string $extra): void
    {
        $numbers = [1, 1.5, '123'];
        $key = array_rand($numbers);
        Scaffolding\takesNumeric($numbers[$key]);     // 1|1.5|'123' — numeric on every branch
        Scaffolding\takesNumeric($numbers[2]);        // '123' at the position it was written

        $grades = ['a', 'b', 'c'];
        foreach ($grades as $grade) {
            Scaffolding\takesGrade($grade);           // 'a'|'b'|'c', not plain string
        }

        $mutated = [1, 1.5, '123'];
        $mutated[] = $extra;              // list<1|1.5|string> — the push widens
        Scaffolding\takesScalar($mutated[0]);
    }

    /**
     * Sibling branches writing the same key merge into one array rather
     * than one cumulative snapshot per branch, so the inferred type
     * matches the declared one.
     *
     * @param list<Scaffolding\Pen|Scaffolding\Pencil> $tools
     * @return array<int, array{pen: Scaffolding\Pen}|array{pencil: Scaffolding\Pencil}>
     */
    public function branchedKeyWrites(array $tools): array
    {
        $rows = [];
        $slot = 0;
        foreach ($tools as $tool) {
            if ($tool instanceof Scaffolding\Pen) {
                $rows[$slot] = ['pen' => $tool];
            }
            if ($tool instanceof Scaffolding\Pencil) {
                $rows[$slot] = ['pencil' => $tool];
            }
            $slot++;
        }

        return $rows;                     // array<int, array{pen: Scaffolding\Pen}|array{pencil: Scaffolding\Pencil}>
    }

    /**
     * A trailing `[]` appends to the level the key chain reaches, and the
     * levels above it are created as the write walks through them, so the
     * pushed values are still known when they are read back.
     *
     * @param list<Scaffolding\Pen> $pens
     * @return array<int, list<Scaffolding\Pen>>
     */
    public function appendsBelowAKey(array $pens): array
    {
        $bySlot = [];
        $byName = [];
        foreach ($pens as $slot => $pen) {
            $bySlot[$slot][] = $pen;      // array<int, list<Scaffolding\Pen>>
            $byName['all'][] = $pen;      // array{all?: list<Scaffolding\Pen>}
        }

        foreach ($byName['all'] ?? [] as $named) {
            $named->write();              // Scaffolding\Pen — the push is what it holds
        }

        return $bySlot;
    }

    /**
     * PHP's `+` on two arrays keeps every key the left side already holds
     * and adds only the ones the right side contributes, so the merged
     * shape still knows what sits under each key.
     *
     * @return array{tool: Scaffolding\Pen, spare: Scaffolding\Pencil}
     */
    public function unionsTwoShapes(): array
    {
        $kit = ['tool' => new Scaffolding\Pen()];
        $kit += ['spare' => new Scaffolding\Pencil(), 'tool' => new Scaffolding\Pencil()];

        $kit['tool']->write();            // Scaffolding\Pen — the left key wins
        $kit['spare']->sketch();          // Scaffolding\Pencil — added by the right

        return $kit;
    }

    /**
     * A write refines what the array already holds rather than replacing
     * it. An append takes the next free key beside the keys already
     * tracked, an append below a key extends what that key holds, and a
     * write into an array declared by key and value type leaves both
     * standing — the keys it held before the write are still there.
     *
     * @param array<string, Scaffolding\Pen> $byName
     * @return array{tool: Scaffolding\Pen, Scaffolding\Pencil}
     */
    public function writesRefineTheTrackedArray(array $byName, string $name): array
    {
        $kit = ['tool' => new Scaffolding\Pen()];
        $kit[] = new Scaffolding\Pencil();

        $kit['tool']->write();            // Scaffolding\Pen — kept across the append
        $kit[0]->sketch();                // Scaffolding\Pencil — the appended slot

        $shelf = ['pens' => [new Scaffolding\Pen()]];
        $shelf['pens'][] = new Scaffolding\Pen();
        $shelf['pens'][1]->write();       // Scaffolding\Pen — the append is what it holds

        $byName['spare'] = new Scaffolding\Pen();
        $byName[$name]->write();          // Scaffolding\Pen — every other key survives the write

        return $kit;
    }
}


// ── A Tag on the Docblock's Opening Line ────────────────────────────────────

class OpeningLineTagDemo
{
    /**
     * A docblock may start its first tag on the `/**` line itself instead of
     * on a continuation line below it. Both spellings say the same thing, so
     * the tag is read either way and the parameter holds what it declares
     * rather than falling back to the wider native hint.
     */
    public function demo(): void
    {
        // Try: hover `$mark` inside `Scaffolding\scaffoldingGradeOnOpeningLine()`.
        var_dump(Scaffolding\scaffoldingGradeOnOpeningLine('b'));   // 'a'|'b'|'c', not plain string

        // Try: complete `$tool->` inside `Scaffolding\scaffoldingWriteAllOnOpeningLine()`.
        var_dump(Scaffolding\scaffoldingWriteAllOnOpeningLine([new Scaffolding\Pen(), new Scaffolding\Pen()]));
    }
}


// ── Named Key Destructuring from Array Shapes ───────────────────────────────

class DestructuringShapeDemo
{
    public function demo(): void
    {
        // Named key from method return
        ['pen' => $pen, 'pencil' => $pencil] = $this->getToolKit();
        $pen->write();                    // Scaffolding\Pen from 'pen' key
        $pencil->sketch();                // Scaffolding\Pencil from 'pencil' key

        // Named key from @var annotated variable
        /** @var array{pen: Scaffolding\Pen, pencil: Scaffolding\Pencil, active: bool} $data */
        $data = Scaffolding\getUnknownValue();
        ['pen' => $myPen, 'pencil' => $myPencil] = $data;
        $myPen->write();                  // Scaffolding\Pen from 'pen' key
        $myPencil->sketch();              // Scaffolding\Pencil from 'pencil' key

        // Positional from shape
        /** @var array{Scaffolding\Pen, Scaffolding\Pencil} $pair */
        $pair = Scaffolding\getUnknownValue();
        [$first, $second] = $pair;
        $first->write();                  // Scaffolding\Pen (positional index 0)
        $second->sketch();                // Scaffolding\Pencil (positional index 1)

        // Positional shape indexed directly with an integer literal
        $pair[0]->write();                 // Scaffolding\Pen (positional index 0)
        $pair[1]->sketch();                // Scaffolding\Pencil (positional index 1)

        // Positional shape spread across multiple docblock lines
        /**
         * @var array{
         *     Scaffolding\Pen,
         *     Scaffolding\Pencil,
         * } $multiline
         */
        $multiline = Scaffolding\getUnknownValue();
        $multiline[0]->write();            // Scaffolding\Pen (positional index 0)
        $multiline[1]->sketch();           // Scaffolding\Pencil (positional index 1)

        // list() syntax
        /** @var array{recipe: Scaffolding\Recipe, servings: int} $meal */
        $meal = Scaffolding\getUnknownValue();
        list('recipe' => $recipe) = $meal;
        $recipe->ingredients;             // Scaffolding\Recipe from 'recipe' key
    }

    /** @return array{pen: Scaffolding\Pen, pencil: Scaffolding\Pencil, count: int} */
    public function getToolKit(): array { return []; }

    public function inferredTuples(): void
    {
        // Inferred (unannotated) nested array literals keep their positional
        // arity, so the foreach element is a fixed tuple and integer-literal
        // indexing resolves each position.
        $rows = [[new Scaffolding\Pen(), new Scaffolding\Pencil()]];
        foreach ($rows as $row) {
            $row[0]->write();             // Scaffolding\Pen (nested tuple index 0)
            $row[1]->sketch();            // Scaffolding\Pencil (nested tuple index 1)
        }

        // A heterogeneous tuple indexed at a position that only some arms
        // have, combined with a `?? Class::class` fallback, keeps the value
        // a class-string instead of widening to plain string.
        $specs = [['pen', Scaffolding\Pen::class], ['pencil']];
        foreach ($specs as $spec) {
            $toolClass = $spec[1] ?? Scaffolding\Pencil::class;   // class-string<Scaffolding\Pen>|class-string<Scaffolding\Pencil>
            $tool = new $toolClass();
            $tool->label();               // Scaffolding\Pen|Scaffolding\Pencil created from the class-string
        }

        // A literal written straight into a variable keeps its arity too,
        // so a row pushed into a collection destructures back into the
        // values it was written with rather than the union of all of them.
        $entries = [];
        $entries[] = [new Scaffolding\Pen(), 'sketchbook'];
        foreach ($entries as $entry) {
            [$writer, $surface] = $entry;
            $writer->write();             // Scaffolding\Pen (slot 0, not Pen|string)
            strlen($surface);             // string (slot 1, not Pen|string)
        }
    }

    public function runtimeArrayKeys(string $slot): void
    {
        // A key PHP only works out at runtime names no shape field, so the
        // literal is described by the key and value types it does have.
        $bySlot = [$slot => new Scaffolding\Pen()];   // array<string, Scaffolding\Pen>
        foreach ($bySlot as $held) {
            $held->write();               // Scaffolding\Pen
        }

        // `+` keeps the left side's keys and adds the right side's, the
        // same union `+=` performs.
        $merged = ['pen' => new Scaffolding\Pen()] + ['pencil' => new Scaffolding\Pencil()];
        $merged['pen']->write();          // Scaffolding\Pen from the left operand
        $merged['pencil']->sketch();      // Scaffolding\Pencil from the right operand

        // Casting an empty array gives the property-less stdClass PHP
        // builds, not an object shape.
        $bare = (object) [];              // stdClass
        echo get_class($bare);
    }
}


// ── Generic Context Preservation ────────────────────────────────────────────

class GenericContextDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingGenericContext();

        $src->chest->unwrap()->open();             // Scaffolding\Box<Scaffolding\Gift>::unwrap() → Scaffolding\Gift
        $src->display()->first()->open();          // Scaffolding\TypedCollection<int, Scaffolding\Gift>::first() → Scaffolding\Gift
    }
}


// ── Generic Shape Substitution ──────────────────────────────────────────────

class GenericShapeDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingGenericShape();

        // Template params inside array shape bodies are substituted through inheritance
        $result = $src->getResult();
        $result['data']->open();          // array{data: T} with T=Scaffolding\Gift → Scaffolding\Gift

        // Chained bracket access walks shape key then list element
        $first = $result['items'][0];
        $first->open();                   // list<T> with T=Scaffolding\Gift → Scaffolding\Gift
    }
}


// ── Foreach, Key Types, and Destructuring ───────────────────────────────────

class IterationDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingIteration();

        // From method
        foreach ($src->allPens() as $pen) {
            $pen->write();                // list<Scaffolding\Pen> → Scaffolding\Pen
        }

        // From property
        foreach ($src->batch as $pen) {
            $pen->write();
        }

        // Key types
        foreach ($src->crossRef() as $pen => $pencil) {
            $pen->write();                // Scaffolding\Pen (key type)
            $pencil->sketch();            // Scaffolding\Pencil (value type)
        }

        // Scalar key types: the key comes from what is being iterated
        foreach ($src->allPens() as $listKey => $listPen) {
            abs($listKey);                // int (list keys are ints, not int|string)
            $listPen->write();
        }
        /** @var array{tool: Scaffolding\Pen, spare: Scaffolding\Pen} $kit */
        $kit = ['tool' => new Scaffolding\Pen(), 'spare' => new Scaffolding\Pen()];
        foreach ($kit as $shapeKey => $shapePen) {
            strlen($shapeKey);            // string (this shape's keys are all strings)
            $shapePen->write();
        }

        // A `Pen[]` shorthand names only the value type, so its keys are
        // whatever PHP allows. Passing one to `strlen()` is accepted because
        // the union stands in for an annotation nobody wrote, and `is_int()`
        // narrows the rest of the loop to the other half.
        foreach ($src->openKeyed() as $openKey => $openPen) {
            strlen($openKey);             // int|string (the whole key domain)
            $openPen->write();
            if (is_int($openKey)) {
                continue;
            }
            strtoupper($openKey);         // string (the guard ruled the ints out)
        }

        // Collecting those keys keeps them exactly as lenient: the element
        // type of the array is the same open key domain the loop bound, so
        // an array built out of them is accepted wherever one key would be.
        $collectedKeys = [];
        foreach ($src->openKeyed() as $collectedKey => $collectedPen) {
            $collectedKeys[] = $collectedKey; // list<int|string>
            $collectedPen->write();
        }
        implode(', ', $collectedKeys);

        // WeakMap keys
        /** @var \WeakMap<Scaffolding\Pen, Scaffolding\Pencil> $mapping */
        $mapping = new \WeakMap();
        foreach ($mapping as $pen => $pencil) {
            $pen->write();                // key: Scaffolding\Pen
            $pencil->sketch();            // value: Scaffolding\Pencil
        }

        // Destructuring
        [$first, $second] = $src->allPens();
        $first->write();                  // destructured element type

        // Foreach destructuring
        /** @var array<int, array{string, int}> $rows */
        $rows = [['Alice', 30], ['Bob', 25]];
        foreach ($rows as [$name, $age]) {
            strlen($name);                // string from positional shape
            abs($age);                    // int from positional shape
        }

        // Foreach keyed shape destructuring
        /** @var array<int, array{tool: Scaffolding\Pen, count: int}> $inv */
        $inv = [];
        foreach ($inv as ['tool' => $tool, 'count' => $count]) {
            $tool->write();               // Scaffolding\Pen from keyed shape
            abs($count);                  // int from keyed shape
        }

        // Nested destructuring
        /** @var array{string, array{Scaffolding\Pen, Scaffolding\Pencil}} $nested */
        $nested = ['label', [new Scaffolding\Pen(), new Scaffolding\Pencil()]];
        [$label, [$nestedPen, $nestedPencil]] = $nested;
        strlen($label);                   // string from outer position 0
        $nestedPen->write();              // Scaffolding\Pen from inner position 0
        $nestedPencil->sketch();          // Scaffolding\Pencil from inner position 1

        // Skipped positions still count: a hole names nothing but occupies
        // its slot, so everything after it shifts along with it.
        /** @var array{string, Scaffolding\Pen, Scaffolding\Pencil} $triple */
        $triple = ['label', new Scaffolding\Pen(), new Scaffolding\Pencil()];
        [, $skippedPen, ] = $triple;
        $skippedPen->write();             // Scaffolding\Pen from position 1
        [, , $skippedPencil] = $triple;
        $skippedPencil->sketch();         // Scaffolding\Pencil from position 2

        /** @var array<int, array{string, Scaffolding\Pen}> $labelled */
        $labelled = [];
        foreach ($labelled as [, $rowPen]) {
            $rowPen->write();             // Scaffolding\Pen from position 1
        }
    }
}


// ── Foreach Array Shape Elements ────────────────────────────────────────────

class ForeachArrayShapeDemo
{
    /**
     * @param array<int, array{tool: Scaffolding\Pen, count: int}> $inventory
     */
    public function demo(array $inventory): void
    {
        // When iterating over an array whose value type is an array shape,
        // the foreach variable carries the shape type so that bracket
        // access resolves each key to its declared type.
        foreach ($inventory as $entry) {
            $entry['tool']->write();      // array{tool: Scaffolding\Pen, count: int} → Scaffolding\Pen
        }
    }
}


// ── Variadic Parameter Foreach ──────────────────────────────────────────────

class VariadicForeachDemo
{
    public function demo(Scaffolding\Pen ...$pens): void
    {
        // Variadic parameters are arrays: foreach extracts the element type
        foreach ($pens as $pen) {
            $pen->write();                // element type from variadic Scaffolding\Pen ...$pens
        }
    }

    public function unionVariadic(Scaffolding\Pen|Scaffolding\Pencil ...$tools): void
    {
        // Union variadic: foreach value is Scaffolding\Pen|Scaffolding\Pencil
        foreach ($tools as $tool) {
            if ($tool instanceof Scaffolding\Pen) {
                $tool->write();           // narrowed to Scaffolding\Pen via instanceof
            }
        }
    }
}


// ── Array Function Type Preservation ────────────────────────────────────────

class ArrayFuncDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingArrayFunc();

        // array_filter keeps the key of every entry it keeps, so filtering a
        // list leaves gaps in the numbering: the result is array<int, Pen>
        // and its first entry need not be at key 0. The element type is what
        // survives the call.
        $active = array_filter($src->members, fn(Scaffolding\Pen $pen) => $pen->color() === 'blue');
        $renumbered = array_values($active);
        $renumbered[0]->write();          // Scaffolding\Pen preserved through array_filter

        $vals = array_values($src->members);
        $vals[0]->write();                // Scaffolding\Pen preserved through array_values

        $pens = $src->roster();
        $last = array_pop($pens);
        $last->write();                   // single Scaffolding\Pen from array_pop

        $cur = current($src->members);
        $cur->write();                    // Scaffolding\Pen from current()

        end($src->members)->write();      // inline end() without variable

        foreach (array_filter($src->members, fn(Scaffolding\Pen $pen) => true) as $pen) {
            $pen->color();                // Scaffolding\Pen preserved in foreach
        }

        $mapped = array_map(fn($pen) => $pen, $src->members);
        $mapped[0]->write();              // Scaffolding\Pen from array_map fallback

        // The same rules apply without an intermediate variable.
        array_values(array_filter($src->members))[0]->write();
        array_map(fn($pen): Scaffolding\Pen => $pen, $src->members)[0]->write();

        // Untyped callback parameter inferred from a method-call array
        // argument: `$pen` resolves to Scaffolding\Pen from roster()'s list<Scaffolding\Pen>.
        array_map(fn($pen) => $pen->color(), $src->roster());
        array_filter($src->roster(), fn($pen) => $pen->color() === 'blue');

        // array_reduce: return type inferred from initial value (3rd arg)
        $merged = array_reduce($src->members, function(Scaffolding\Pen $carry, Scaffolding\Pen $item): Scaffolding\Pen {
            return $carry;
        }, new Scaffolding\Pen('merged'));
        $merged->write();                 // Scaffolding\Pen from initial value argument

        // array_sum / array_product: always int|float
        $total = array_sum([10, 20, 30]);
        $product = array_product([2, 3, 4]);
    }

    /**
     * A callback may declare the widest hint PHP has a keyword for; the
     * element shape from the call site still narrows it.
     *
     * @return list<string>
     */
    public function pairColors(Scaffolding\ScaffoldingArrayFunc $src): array
    {
        return array_map(
            static fn(array $pair): string => $pair[0]->color(),  // Scaffolding\Pen from array{Scaffolding\Pen, string}
            $src->pairs()
        );
    }

    /**
     * The key- and value-reading builtins answer in terms of the array they
     * were handed, rather than in the widest thing their signature can say.
     *
     * Try: trigger completion after each `->` below.
     */
    public function keysAndValues(Scaffolding\ScaffoldingArrayFunc $src): void
    {
        $names = array_keys($src->byName());
        strtoupper($names[0]);            // list<string>: the key type, no int branch

        $indices = array_keys($src->labels());
        intdiv($indices[0], 1);           // list<int>: a list is int-keyed

        $pens = array_values($src->byName());
        $pens[0]->write();                // list<Scaffolding\Pen>: renumbered, not keyed

        // int|false, since the haystack is a list: no string branch to rule out.
        $found = array_search('gel', $src->labels());
        if ($found !== false) {
            intdiv($found, 1);            // int
        }

        // string|null: the key type again, never int.
        $firstName = array_key_first($src->byName());
        if ($firstName !== null) {
            strtoupper($firstName);       // string
        }

        // A scalar element survives the preserving family the same way a
        // Scaffolding\Pen does.
        $unique = array_unique($src->labels());
        strtoupper($unique[0]);           // list<string>

        $labels = $src->labels();
        $label = array_pop($labels);
        strtoupper($label);               // string, not mixed

        // With no callback, array_filter keeps only the truthy entries, so
        // the null half of the value type is gone.
        $present = array_filter($src->optionalLabels());
        strtoupper($present['ink']);      // array<string, string>

        // ARRAY_FILTER_USE_KEY hands the callback the key, so what the
        // callback proves about it describes the keys that survive.
        $stringKeyed = array_filter($src->mixedKeys(), fn($key) => is_string($key), ARRAY_FILTER_USE_KEY);
        strtoupper(array_keys($stringKeyed)[0]);   // list<string>: the int key is gone

        // The default mode hands the callback the value instead, so a
        // callback that tests it says as much about the entries that
        // survive as the truthiness test above does.
        $named = array_filter($src->optionalLabels(), fn($label) => $label !== null);
        strtoupper($named['ink']);        // array<string, string>: the null half is gone

        // An instanceof check filters for a type, and the result carries it.
        // array_filter keeps the original keys, so array_values renumbers
        // them before the first entry is read back.
        $pens = array_values(array_filter($src->mixedWriters(), fn($writer) => $writer instanceof Scaffolding\Pen));
        $pens[0]->write();                // list<Scaffolding\Pen>, so Pen's members are here

        // An all-int array cannot sum to a float.
        $total = array_sum($src->weights());
        intdiv($total, 1);                // int

        // array_merge is the one member of the family that concatenates
        // rather than rearranges, so every argument contributes to the
        // element type. The [] an accumulator starts as contributes nothing,
        // which is what keeps the loop's result typed.
        $collected = [];
        foreach ([$src->roster(), $src->roster()] as $batch) {
            $collected = array_merge($collected, $batch);
        }
        $collected[0]->write();           // list<Scaffolding\Pen>

        // Two different element types union, so only what they share is here.
        $writers = array_merge($src->roster(), $src->mixedWriters());
        $writers[0]->label();             // list<Scaffolding\Pen|Scaffolding\Pencil>

        // PHP renumbers integer keys as it appends and carries string keys
        // over, so merging a list into a string-keyed array holds both.
        $everyone = array_merge($src->roster(), $src->byName());
        $everyone['blue']->write();       // array<int|string, Scaffolding\Pen>
    }

    /** @return array{names: list<string>, first: string, total: int} */
    public function keyValueSummary(Scaffolding\ScaffoldingArrayFunc $src): array
    {
        $present = array_filter($src->optionalLabels());

        return [
            'names' => array_keys($src->byName()),
            'first' => array_values($present)[0],
            'total' => array_sum($src->weights()),
        ];
    }

    /**
     * The user-comparison sorts hand their callback two entries of the array
     * they are sorting: two values for usort and uasort, two keys for
     * uksort. Neither the call nor the callback spells the type out.
     *
     * Try: trigger completion after each `->` below.
     */
    public function sortCallbacks(Scaffolding\ScaffoldingArrayFunc $src): void
    {
        $roster = $src->roster();
        usort($roster, static fn($a, $b) => strcmp($a->color(), $b->color()));
        $roster[0]->write();              // list<Scaffolding\Pen> survives the sort

        $byName = $src->byName();
        uasort($byName, static fn($a, $b) => strcmp($a->color(), $b->color()));
        uksort($byName, static fn($a, $b) => strcmp($a, $b));  // $a, $b are the string keys
        $byName['blue']->write();         // array<string, Scaffolding\Pen> survives both
    }

    /**
     * A callback may annotate a return type wider than what its body hands
     * back; the narrower one is what the mapped array holds.
     */
    public function mappedSubclass(Scaffolding\ScaffoldingArrayFunc $src): void
    {
        $renamed = array_map(
            static fn(Scaffolding\Marker $m): Scaffolding\Pen => $m->rename('wide'),
            $src->markers()
        );
        $renamed[0]->highlight();         // Scaffolding\Marker: rename() returns static
    }
}


// ── @throws Completion and Catch Variable Types ─────────────────────────────

class ExceptionDemo extends Scaffolding\ScaffoldingException
{
    public function demo(): void
    {
        try {
            $this->riskyOperation();
        } catch (Scaffolding\ValidationException $e) {
            $e->getMessage();             // catch variable resolves exception type
        }
    }

    /**
     * Typing `@` in this docblock suggests @throws for each uncaught exception.
     *
     * @throws Scaffolding\NotFoundException
     * @throws Scaffolding\ValidationException
     */
    public function findOrFail(int $id): array
    {
        if ($id < 0) {
            throw new Scaffolding\ValidationException('ID must be positive');
        }
        $result = $this->lookup($id);
        if ($result === null) {
            throw new Scaffolding\NotFoundException('Record not found');
        }
        return $result;
    }

    /**
     * Caught exceptions are filtered out of @throws suggestions.
     *
     * @throws Scaffolding\AuthorizationException
     */
    public function safeOperation(): void
    {
        try {
            throw new \RuntimeException('transient error');
        } catch (\RuntimeException $e) {
            // caught — not suggested
        }
        throw new Scaffolding\AuthorizationException('Forbidden');
    }

    /**
     * Called method's @throws propagate to the caller.
     *
     * @throws Scaffolding\AuthorizationException
     */
    public function delegatedWork(): void
    {
        $this->safeOperation();
    }
}


// ── Constructor @param → Promoted Property Override ─────────────────────────

class ParamOverrideDemo
{
    public function demo(): void
    {
        foreach ($this->ingredients as $ingredient) {
            $ingredient->name;              // Scaffolding\Ingredient::$name
            $ingredient->format();          // Scaffolding\Ingredient::format()
        }
        $this->recipe->name;                // Scaffolding\Recipe::$name
    }

    /**
     * @param list<Scaffolding\Ingredient> $ingredients
     * @param Scaffolding\Recipe $recipe
     */
    public function __construct(
        public array $ingredients,          // @param overrides to list<Scaffolding\Ingredient>
        public object $recipe,              // @param overrides to Scaffolding\Recipe
    ) {}
}


// ── Inline @var on Promoted Constructor Properties ──────────────────────────

class InlineVarPromotedDemo
{
    public function __construct(
        /** @var array<Scaffolding\Ingredient> */
        public array $ingredients,
    ) {}

    public function demo(): void
    {
        // Inline @var on promoted property overrides the native type hint
        foreach ($this->ingredients as $ingredient) {
            $ingredient->name;              // Scaffolding\Ingredient::$name via inline @var
            $ingredient->format();          // Scaffolding\Ingredient::format() via inline @var
        }
    }
}


// ── Generator / Iterable Yield Type Resolution ─────────────────────────────

class GeneratorDemo
{
    public function demo(): void
    {
        // Generator<int, Scaffolding\Pen> — value is 2nd param (Scaffolding\Pen)
        foreach ($this->getPens() as $pen) {
            $pen->write();                // resolves to Scaffolding\Pen
        }

        // Generator<int, Scaffolding\Pencil, mixed, Scaffolding\Pen> — value is still 2nd param (Scaffolding\Pencil)
        foreach ($this->processPencils() as $pencil) {
            $pencil->sketch();            // Scaffolding\Pencil (2nd param), not Scaffolding\Pen (4th)
        }

        // @var annotated generator
        /** @var \Generator<int, Scaffolding\Pen> $gen */
        $gen = $this->getPens();
        foreach ($gen as $pen) {
            $pen->write();                // Generator<int, Scaffolding\Pen> → Scaffolding\Pen
        }

        // iterable<Scaffolding\Pen> — single param is the value type
        foreach ($this->iterablePens() as $pen) {
            $pen->write();
        }

        // Method chain through generator element
        foreach ($this->getPens() as $pen) {
            $pen->rename('Bold')->color();
        }
    }

    /** @return \Generator<int, Scaffolding\Pen> */
    public function getPens(): \Generator
    {
        yield new Scaffolding\Pen();
    }

    /** @return \Generator<int, Scaffolding\Pencil, mixed, Scaffolding\Pen> */
    public function processPencils(): \Generator
    {
        yield new Scaffolding\Pencil();
    }

    /** @return iterable<Scaffolding\Pen> */
    public function iterablePens(): iterable
    {
        return [];
    }

    /**
     * @param \Generator<int, Scaffolding\Pencil> $pencils
     */
    public function foreachGeneratorParam(\Generator $pencils): void
    {
        foreach ($pencils as $pencil) {
            $pencil->sketch();            // @param overrides native \Generator type
        }
    }
}


// ── Generator Yield Type Inference Inside Bodies ────────────────────────────
//
// Generator<TKey, TValue, TSend, TReturn>
//
// - `yield $expr` produces TValue to the consumer. The yielded variable
//   keeps its own type (from its assignment), not the Generator annotation.
// - `$var = yield $expr` assigns TSend (the sent value) to $var. The yield
//   expression evaluates to whatever was passed via ->send().

class GeneratorYieldDemo
{
    /** @return \Generator<int, Scaffolding\Pen> */
    public function findAll(): \Generator
    {
        // The type of $pen comes from `new Scaffolding\Pen(...)`, not from the @return.
        // Completion on $pen-> works because the assignment is known.
        $pen = new Scaffolding\Pen('blue');
        yield $pen;
        $pen->write();                    // resolves to Scaffolding\Pen

        $anotherPen = new Scaffolding\Pen('red');
        yield 0 => $anotherPen;
        $anotherPen->color();             // key => value yields also work
    }

    /** @return \Generator<int, Scaffolding\Pen> */
    public function yieldInsideControlFlow(): \Generator
    {
        if (true) {
            $pen = new Scaffolding\Pen('green');
            yield $pen;
            $pen->write();                // resolves inside control flow blocks
        }
    }

    /** @return \Generator<int, Scaffolding\Pen> */
    public function chainingThroughYieldInferred(): \Generator
    {
        $pen = new Scaffolding\Pen('black');
        yield $pen;
        $pen->rename('Bold')->color();    // chains through yielded variable
    }

    /** @return \Generator<int, string, Scaffolding\Pencil, void> */
    public function coroutine(): \Generator
    {
        // TSend inference: $var = yield gets the 3rd Generator type param.
        // yield produces 'ready' (TValue = string) to the consumer;
        // the yield expression evaluates to whatever was ->send()'d (TSend = Scaffolding\Pencil).
        $pencil = yield 'ready';
        $pencil->sketch();                // resolves to Scaffolding\Pencil (TSend)
    }

    /** @return \Generator<int, string, Scaffolding\Pencil, void> */
    public function tsendInsideNestedBlocks(): \Generator
    {
        while (true) {
            if (true) {
                $pencil = yield 'waiting';
                $pencil->sketch();        // resolves inside nested blocks
            }
        }
    }
}


// ── Template Parameter Bounds ───────────────────────────────────────────────

/**
 * @template-covariant TNode of Scaffolding\AstNode
 */
class TemplateBoundsDemo
{
    public function demo(): void
    {
        $this->node->getChildren();       // resolves via TNode's bound: Scaffolding\AstNode
        $this->node->getParent();
    }

    /** @var TNode */
    public $node;

    /** @param TNode $node */
    public function __construct(Scaffolding\AstNode $node)
    {
        $this->node = $node;
    }
}


// ── Match Class-String Forwarding to Conditional Return Types ───────────────

class MatchClassStringDemo
{
    public function demo(): void
    {
        $container = new Scaffolding\Container();

        // Match expression → class-string → conditional return
        $requestType = match (rand(0, 1)) {
            0 => Scaffolding\ElasticProductReviewIndexService::class,
            1 => Scaffolding\ElasticBrandIndexService::class,
        };
        $requestBody = $container->make($requestType);
        $requestBody->index();            // on both classes
        $requestBody->reindex();          // Scaffolding\ElasticProductReviewIndexService only

        // Standalone function with @template
        $resolved = Scaffolding\resolve($requestType);
        $resolved->index();               // on both classes

        // Inline chain
        $container->make($requestType)->index();

        // Simple class-string variable
        $cls = Scaffolding\Pen::class;
        $pen = $container->make($cls);
        $pen->write();                    // resolves through simple $cls variable

        // Ternary class-string
        $ternary = rand(0, 1) ? Scaffolding\Pen::class : Scaffolding\Pencil::class;
        $obj = $container->make($ternary);
        $obj->label();                    // shared by both types
    }
}


// ── Closure Parameter Inference ─────────────────────────────────────────────

class ClosureParamInferenceDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingClosureParamInference();

        // $p is inferred as Scaffolding\Pen from map's callable(TValue, TKey) signature
        $src->items->map(fn($p) => $p->write());

        // Closure body
        $src->items->each(function ($pen) {
            $pen->write();                // resolves to Scaffolding\Pen
        });

        // Explicit type hint takes precedence over inference
        $src->items->map(fn(Scaffolding\Pencil $p) => $p->sketch());

        // $this in callable param resolves to receiver, not current class
        $pipeline = new Scaffolding\ScaffoldingPipeline();
        $pipeline->when(true, function ($pipe) {
            $pipe->send('data');          // resolves to Scaffolding\ScaffoldingPipeline, not this demo class
        });

        // Arrow function variant
        $pipeline->tap(fn($p) => $p->through([]));

        // Function-level @template callable inference
        // array_any(@param array<TKey, TValue>, @param callable(TValue, TKey): bool)
        // $item is inferred as Scaffolding\Pen from the array's element type via template substitution
        $holder = new Scaffolding\ScaffoldingTemplateCallableHolder();
        array_any($holder->tools, fn($item) => $item->write() !== '');
    }

    /**
     * A declared closure-parameter union is preserved even when the
     * subject is a union of differently parameterized collections.
     *
     * @param Scaffolding\FluentCollection<int, Scaffolding\Pen>|Scaffolding\FluentCollection<int, Scaffolding\Pencil> $tools
     */
    public function unionSubject(Scaffolding\FluentCollection $tools): void
    {
        // $item resolves to the declared union Scaffolding\Pen|Scaffolding\Pencil, not just the
        // first collection's element type (Scaffolding\Pen).  label() exists on both,
        // so it resolves; hover on $item shows Scaffolding\Pen|Scaffolding\Pencil.
        $tools->each(function (Scaffolding\Pen|Scaffolding\Pencil $item): void {
            $item->label();               // resolves on both union arms
        });
    }

    /**
     * A doc comment written above the statement a closure is assigned in
     * still types the closure. PHP attaches the comment to the statement,
     * but its `@param` tags describe the closure's own parameters.
     *
     * Try: trigger completion after `$pen->` inside the closure body.
     */
    public function docblockedClosure(): string
    {
        /**
         * @param list<Scaffolding\Pen> $pens
         */
        $labels = static function (array $pens): string {
            $out = '';
            foreach ($pens as $pen) {
                $out .= $pen->color();    // Scaffolding\Pen from the docblock above the assignment
            }

            return $out;
        };

        return $labels([new Scaffolding\Pen('blue')]);
    }
}


// ═══════════════════════════════════════════════════════════════════════════
//  TRIVIAL — works in most editors, included for completeness
// ═══════════════════════════════════════════════════════════════════════════


// ── Static & Enum Completion ────────────────────────────────────────────────

class StaticEnumDemo
{
    /** The receiver is a property, so `self` below binds to Scaffolding\Status, not to this class. */
    private Scaffolding\Status $state = Scaffolding\Status::Pending;

    public function transitions(): void
    {
        $this->state->canChangeTo(Scaffolding\Status::Active);   // self parameter accepts a Scaffolding\Status
        Scaffolding\Status::Pending->canChangeTo(Scaffolding\Status::Active);
    }

    public function demo(): void
    {
        Scaffolding\User::$defaultRole;          // static property
        Scaffolding\User::TYPE_ADMIN;            // class constant
        Scaffolding\User::findByEmail('a@b.c');  // static method
        Scaffolding\User::make('Bob');           // inherited static (Scaffolding\Model)
        Scaffolding\User::query();               // @mixin Scaffolding\Builder (Scaffolding\Model)

        Scaffolding\Status::Active;              // backed enum case
        Scaffolding\Status::Active->label();     // enum method
        Scaffolding\Status::Active->name;        // "Active" (from UnitEnum)
        Scaffolding\Status::Active->value;       // "active" (from BackedEnum)
        Scaffolding\Priority::High;              // int-backed enum
        Scaffolding\Priority::High->name;        // "High" (from UnitEnum)
        Scaffolding\Priority::High->value;       // 3 (from BackedEnum, int)
        Scaffolding\Mode::Manual;                // unit enum
        Scaffolding\Mode::Manual->name;          // "Manual" (from UnitEnum)

        // Enum case assigned to variable
        $status = Scaffolding\Status::Active;
        $status->name;               // resolves through variable
        $status->value;

        // self::/static:: inside enum methods resolve to the enum type
        Scaffolding\Status::defaultValue();      // self::Active->value inside enum

        // cases() returns a list of the enum's own instances, so indexing
        // it inline resolves the element back to the enum.
        Scaffolding\Status::cases()[0]->value;   // "active" — cases()[0] is a Scaffolding\Status
        Scaffolding\Priority::cases()[0]->name;  // "Low" — cases()[0] is a Scaffolding\Priority
    }
}


// ── Callable Snippet Insertion ──────────────────────────────────────────────

class SnippetInsertionDemo
{
    public function demo(): Scaffolding\Response
    {
        // Completion inserts snippets with tab-stops for required params
        $user = new Scaffolding\User('Alice', 'alice@example.com');
        $user->setName('Bob');                    // → setName(${1:$name})
        $user->toArray();                         // → toArray()  (no params)
        $user->addRoles();                        // → addRoles() (variadic — no tab-stops)
        Scaffolding\User::findByEmail('a@b.c');               // → findByEmail(${1:$email})
        return new Scaffolding\Response(200);                 // → Scaffolding\Response(${1:$statusCode})
    }
}


// ── Context-Aware Class Name Filtering ──────────────────────────────────────
// Try: erase the class name after each keyword and re-trigger completion.
//
// extends Scaffolding\Model        → classes only, non-final
//                        MUST show: Scaffolding\User, Scaffolding\Response, Scaffolding\Pen (non-final classes)
//                        MUST NOT show: Scaffolding\AdminUser (final), Scaffolding\Model (abstract),
//                        Scaffolding\Renderable (interface), Scaffolding\HasTimestamps (trait), Scaffolding\Status (enum)
//
// extends Scaffolding\Renderable   → interfaces only (interface-extends-interface)
//                        MUST show: Scaffolding\Renderable, GtdShape (definition.php), Scaffolding\Printable
//                        MUST NOT show: Scaffolding\User (class), Scaffolding\HasTimestamps (trait), Scaffolding\Status (enum)
//
// implements Scaffolding\Renderable → interfaces only
//                        MUST show: Scaffolding\Renderable, GtdShape (definition.php), Scaffolding\Printable
//                        MUST NOT show: Scaffolding\User (class), Scaffolding\HasTimestamps (trait), Scaffolding\Status (enum)
//
// use Scaffolding\HasTimestamps    → traits only (inside class body)
//                        MUST show: Scaffolding\HasTimestamps, Scaffolding\HasSlug, Scaffolding\JsonSerializer
//                        MUST NOT show: Scaffolding\User (class), Scaffolding\Renderable (interface), Scaffolding\Status (enum)
//
// instanceof Scaffolding\User      → classes, interfaces, enums (no traits)
//                        MUST show: Scaffolding\User, Scaffolding\Renderable, Scaffolding\Status
//                        MUST NOT show: Scaffolding\HasTimestamps (trait)
//
// new Scaffolding\User             → concrete non-abstract classes only
//                        MUST show: Scaffolding\User, Scaffolding\Pen, Scaffolding\Response
//                        MUST NOT show: Scaffolding\Model (abstract), Scaffolding\AdminUser (final is ok for new),
//                        Scaffolding\Renderable (interface), Scaffolding\HasTimestamps (trait), Scaffolding\Status (enum)

class ClassFilteringDemo extends Scaffolding\Model implements Scaffolding\Renderable
{
    use Scaffolding\HasTimestamps;
    public function test(): bool { return $this instanceof Scaffolding\User; }
    public function format(string $template): string { return ''; }
    public function toArray(): array { return []; }
}


// ── Type Hint Completion in Definitions ─────────────────────────────────────
// Try: trigger completion when typing a type hint — PHP scalars (string,
// int, float, bool) appear alongside class names, with no constants or
// functions in the list. Traits are excluded because they cannot be used
// as type hints in PHP (the type check always fails at runtime).
//
// The same filtering applies in PHPDoc type positions: @param, @return,
// and @var exclude traits, while @throws uses Throwable-filtered
// completion (only exception classes and Throwable interfaces).

function typeHintDemo(Scaffolding\User $user, string $name): string { return $user->displayName . $name; }

function unionDemo(string|int $value, ?Scaffolding\User $maybe): string { return $maybe . $maybe->displayName; }


// ── $_SERVER Superglobal ────────────────────────────────────────────────────

class ServerSuperglobalDemo
{
    public function demo(): void
    {
        $_SERVER[''];   // Try: key completion for REQUEST_METHOD, HTTP_HOST, etc.
    }
}


// ═══════════════════════════════════════════════════════════════════════════
//  ADVANCED — specialized features
// ═══════════════════════════════════════════════════════════════════════════


// ── Intersection Types ──────────────────────────────────────────────────────

class IntersectionDemo
{
    public function demo(Scaffolding\Envelope&Scaffolding\Printable $item): void
    {
        $item->print();                       // from Scaffolding\Printable
        $item->seal();                        // from Scaffolding\Envelope
    }

    /**
     * A parenthesized "DNF" return type `(A&B)|null` resolves to the
     * intersection instead of being discarded, so after a null check
     * both interfaces' members are available on the result.
     *
     * @return (Scaffolding\Envelope&Scaffolding\Printable)|null
     */
    public function sealed(): ?Scaffolding\Envelope
    {
        return Scaffolding\openSealedEnvelope();
    }

    public function useSealed(): void
    {
        $item = $this->sealed();
        $item?->print();                      // from Scaffolding\Printable, via the DNF return type
        $item?->seal();                       // from Scaffolding\Envelope
    }
}


// ── Ternary Narrowing ──────────────────────────────────────────────────────

class TernaryNarrowingDemo
{
    public function __construct(private Scaffolding\Pen|Scaffolding\Pencil $tool) {}

    public function demo(): void
    {
        // Variable subject: narrowed to Scaffolding\Rock (then) / Scaffolding\Banana (else)
        $thing = Scaffolding\pickRockOrBanana();
        $thing instanceof Scaffolding\Rock ? $thing->crush() : $thing->peel();
    }

    /**
     * instanceof in a ternary condition narrows the `$this->tool` property
     * subject inside the then-branch, so `->write()` (declared only on Scaffolding\Pen)
     * resolves. Because the then-branch resolves, the ternary type is the
     * union of both branches (`string|null`), not just the else-branch.
     */
    public function toolLabel(): ?string
    {
        return $this->tool instanceof Scaffolding\Pen
            ? $this->tool->write()            // narrowed to Scaffolding\Pen inside the ternary
            : null;
    }

    /**
     * A truthy ternary condition narrows a repeated nullable method-call
     * subject to its non-null type inside the then-branch.
     */
    public function repeatedCall(): ?string
    {
        return $this->maybePen()
            ? $this->maybePen()->write()      // narrowed to Scaffolding\Pen (null stripped)
            : null;
    }

    private function maybePen(): ?Scaffolding\Pen
    {
        return $this->tool instanceof Scaffolding\Pen ? $this->tool : null;
    }
}


// ── Class Alias ─────────────────────────────────────────────────────────────

class ClassAliasDemo
{
    public function demo(): void
    {
        $profile = new Profile(new Scaffolding\User('Eve', 'eve@example.com'));
        $profile->getDisplayName();               // Profile → Scaffolding\UserProfile via `use ... as`
    }
}


// ── self::class / static::class ─────────────────────────────────────────────

class SelfClassDemo
{
    public function demo(): string
    {
        return self::class;          // resolves to SelfClassDemo
    }
}


// ── Trait insteadof / as Conflict Resolution ────────────────────────────────

class TraitConflictDemo
{
    use Scaffolding\JsonSerializer, Scaffolding\XmlSerializer {
        Scaffolding\JsonSerializer::serialize insteadof Scaffolding\XmlSerializer;
        Scaffolding\XmlSerializer::serialize as serializeXml;
        Scaffolding\JsonSerializer::serialize as private internalSerialize;
    }

    public function demo(): void
    {
        $this->internalSerialize();       // aliased as private
        $this->serialize();               // Scaffolding\JsonSerializer wins via insteadof
        $this->serializeXml();            // Scaffolding\XmlSerializer::serialize aliased
        $this->toJson();                  // non-conflicting from Scaffolding\JsonSerializer
        $this->toXml();                   // non-conflicting from Scaffolding\XmlSerializer
    }
}


// ── unset() Tracking ────────────────────────────────────────────────────────

class UnsetDemo
{
    public function demo(): void
    {
        $pen = new Scaffolding\Pen('blue');
        $pen->write();                    // resolves to Scaffolding\Pen
        unset($pen);
        // Try: $pen->  — no completions (variable was unset)

        // Re-assigning after unset restores type
        $tool = new Scaffolding\Pen('red');
        unset($tool);
        $tool = new Scaffolding\Marker();
        $tool->highlight();               // resolves to Scaffolding\Marker

        // unset only affects targeted variable
        $pen2 = new Scaffolding\Pen('green');
        $pencil = new Scaffolding\Pencil();
        unset($pen2);
        $pencil->sketch();                // still resolves to Scaffolding\Pencil
    }
}


// ── First-Class Callable Syntax (PHP 8.1) ───────────────────────────────────

class FirstClassCallableDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingFirstClassCallable();

        $fun = Scaffolding\makePen(...);
        $fun()->write();                   // function reference → Closure returning Scaffolding\Pen

        $orderFn = $src->dispatch(...);
        $orderFn()->write();              // instance method → Closure returning Scaffolding\Pen

        $finder = Scaffolding\Pen::make(...);
        $finder()->color();               // static method → Closure returning Scaffolding\Pen

        $make = Scaffolding\makePen(...);
        $pen = $make();
        $pen->color();                    // assigned result from callable invocation

        // Immediate invocation: method(...)() returns the method's return type
        Scaffolding\makePen(...)()->write();          // function first-class callable invoked immediately
        Scaffolding\Pen::make(...)()->color();        // static method first-class callable invoked immediately
        $src->dispatch(...)()->write();   // instance method first-class callable invoked immediately

        $immediate = Scaffolding\Pen::make(...)();
        $immediate->color();              // assigned result from immediate static callable invocation
    }
}


// ── Array Element Access from Assignments ───────────────────────────────────

class ArrayAccessDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingArrayAccess();

        $pens = $src->fetchAll();         // Scaffolding\Pen[] from method return
        $pens[0]->write();                // resolves to Scaffolding\Pen

        $gifts = (new Scaffolding\ScaffoldingGenericContext())
            ->display();
        $gifts[0]->open();                // resolves to Scaffolding\Gift (element of Scaffolding\Gift[])

        $first = $pens[0];
        $first->color();                  // resolves via $first = $pens[0]

        // Inline method-return array access (no intermediate variable)
        $src->fetchAll()[0]->write();     // resolves Scaffolding\Pen from Scaffolding\Pen[] return type
        $src->fetchAll()[0]->color();     // same, different member
    }
}


// ── Indexing an ArrayAccess Object ───────────────────────────────────────────
// `$obj[$key]` on a class implementing ArrayAccess natively resolves through
// offsetGet(), whether the value type comes from a generic docblock
// annotation or from offsetGet()'s own declared return type.

class ArrayAccessObjectDemo
{
    public function demo(): void
    {
        $pens = new Scaffolding\ScaffoldingPenArrayAccess();
        $pens[0]->write();                // resolves via offsetGet(): Scaffolding\Pen

        $shapes = new Scaffolding\ScaffoldingGenericArrayAccess([new Scaffolding\Pen()]);
        $shapes[0]->write();              // resolves via @implements \ArrayAccess<int, T>, T bound to Scaffolding\Pen
    }
}


// ── Closure / Arrow-Function Members ────────────────────────────────────────

class ClosureMembersDemo
{
    public function demo(): void
    {
        $typedClosure = function(Scaffolding\Pen $pen): string { return $pen->write(); };
        $typedClosure->bindTo($this);     // resolves to Closure::bindTo
        $typedClosure->call($this);       // resolves to Closure::call

        $typedArrow = fn(int $posX): float => $posX * 1.5;
        $typedArrow->bindTo($this);       // resolves to Closure::bindTo

        $fun = function(): void {};
        $bound = $fun->bindTo($this);
        $bound->call($this);             // chained: $bound is still Closure
    }
}


// ── Class-Body Root Member Completion ───────────────────────────────────────
// Triggering completion directly inside a class body (no modifier typed yet)
// suggests every parent/interface/trait member the class can still override
// or implement, inserting the complete declaration: visibility, static, type,
// default value, and full method signatures.  Class names, functions, and
// global constants are invalid at that position and are not offered.

class ClassRootCompletionDemo extends Scaffolding\OverridableWidget
{
    use Scaffolding\WidgetSlug;

    // Try: type `o` on the line below and trigger completion.  Both
    // `$oneTimeToken` and `onChange()` appear with full declarations,
    // even though this comment sits between them and the class brace.

    // Try: type `O` for the inherited constant — `ONE_TIME_TTL` is offered
    // as `public const string ONE_TIME_TTL = '15m';`, keeping its type.

    // Try: type `w` — `withLabel()` from Scaffolding\OverridableWidget declares
    // `@return $this`, which is PHPDoc-only, so the generated override
    // ends in `: static`, never the invalid `: $this`.

    // Try: type `s` — `slug()` comes from the Scaffolding\WidgetSlug trait, whose
    // PHPDoc is NOT inherited by an override, so the completion restates
    // `@param list<string> $parts` and `@return $this` above the method.
    // `slugSeparator()` is offered too: it is `final` in the trait, but a
    // class using the trait may still declare its own version.

    // Try: type `onL` — nothing is offered.  `Scaffolding\OverridableWidget::onLock()`
    // is `final`, so PHP rejects an override of it outright.

    // Try: type `onN` — `$onName` is `readonly` in the parent, and the
    // inserted declaration keeps the modifier: `public readonly string
    // $onName;`.  Redeclaring it without `readonly` is a fatal error.

}


// ── Property-Level Narrowing ────────────────────────────────────────────────

class PropertyNarrowingDemo
{
    private Scaffolding\Pen|Scaffolding\Pencil $tool;

    /** @var Scaffolding\Pen|Scaffolding\Pencil|null */
    public $untyped;

    public function demo(): void
    {
        // instanceof narrows a property inside the then-body
        if ($this->tool instanceof Scaffolding\Pen) {
            $this->tool->write();             // narrowed to Scaffolding\Pen
        }

        // Negated instanceof + early return narrows after the guard
        if (!$this->tool instanceof Scaffolding\Pencil) {
            return;
        }
        $this->tool->sketch();                // narrowed to Scaffolding\Pencil

        // assert() narrows an untyped property
        assert($this->untyped instanceof Scaffolding\Pen);
        $this->untyped->color();              // narrowed to Scaffolding\Pen
    }
}


// ── Pass-by-Reference Parameter Type ────────────────────────────────────────

class PassByReferenceDemo
{
    public function demo(): void
    {
        // When a function takes a typed &$var parameter, the variable
        // acquires that type after the call.
        Scaffolding\initPen($pen);
        $pen->write();                    // $pen is now Scaffolding\Pen

        // Static method calls with by-ref parameters:
        Scaffolding\PenFactory::create($staticPen);
        $staticPen->write();              // $staticPen is now Scaffolding\Pen

        // Constructor calls with by-ref parameters:
        new Scaffolding\PenBuilder($ctorPen);
        $ctorPen->write();                // $ctorPen is now Scaffolding\Pen

        // Instance method calls ($this->method) with by-ref parameters:
        $this->init($thisPen);
        $thisPen->write();                // $thisPen is now Scaffolding\Pen

        // The declared type says what may go *in*. What the callee assigns
        // on every path is what comes back out, so the null `?Pen` allows
        // is gone by the time the call returns and a parameter that will
        // not take null accepts it.
        Scaffolding\initPen($writtenPen);
        Scaffolding\describePen($writtenPen);  // Scaffolding\Pen, never null

        // A callee that only writes on one branch leaves the null in place.
        Scaffolding\initPenWhen(false, $maybePen);
        $maybePen?->write();              // still ?Scaffolding\Pen

        // The value an out-parameter already holds is never checked against
        // the declared type: on the second pass `$matches` still holds the
        // offset-capture shape the first left behind, and `preg_match_all()`
        // overwrites it either way.
        foreach (['a 1', 'b 2'] as $line) {
            preg_match_all('/(\w+)/', $line, $matches, PREG_OFFSET_CAPTURE);
            echo $matches[1][0][1] << 1;  // list<array{string, int<-1, max>}>
        }
    }

    private function init(?Scaffolding\Pen &$pen): void
    {
        $pen = new Scaffolding\Pen();
    }
}


// ── Interface Template Inheritance ──────────────────────────────────────────

class InterfaceTemplateDemo
{
    public function demo(): void
    {
        // When a class implements an interface with @template + class-string<T>,
        // the implementing class inherits the template machinery.
        $locator = new Scaffolding\ScaffoldingEntityLocator();
        $locator->find(Scaffolding\Pen::class)->write();   // T resolves to Scaffolding\Pen via class-string<T>
    }
}


// ── Function-level @template (Scaffolding\collect) ──────────────────────────────────────

class CollectGenericDemo
{
    public function demo(): void
    {
        /** @var Scaffolding\Pen[] $pens */
        $pens = [];

        // Scaffolding\collect() uses function-level @template to carry element types
        // through to the returned Scaffolding\FluentCollection.
        $collection = Scaffolding\collect($pens);
        $collection->first()->write();    // TValue resolves to Scaffolding\Pen

        // Inline chaining works too
        Scaffolding\collect($pens)->first()->write(); // same resolution, no intermediate variable
    }
}


// ── Generic @phpstan-assert Narrowing ───────────────────────────────────────

class GenericAssertNarrowingDemo
{
    public function demo(object $obj): void
    {
        // @phpstan-assert with @template + class-string<T> resolves
        // the narrowed type from the call-site argument.
        Scaffolding\ScaffoldingAssert::assertInstanceOf(Scaffolding\Pen::class, $obj);
        $obj->write();                    // $obj narrowed to Scaffolding\Pen
    }

    public function demoVariableClass(string $cls, ?Scaffolding\Pen $node): void
    {
        // When the asserted class is a variable that cannot be resolved
        // to a concrete class, the assertion narrows to `object`
        // intersected with the prior type: `null` is dropped but the
        // subject keeps the type it already had, so member access still
        // resolves instead of unresolving the subject entirely.
        Scaffolding\ScaffoldingAssert::assertInstanceOf($cls, $node);
        $node->write();                   // $node kept as Scaffolding\Pen (null dropped)
    }
}


// ── @param-closure-this ─────────────────────────────────────────────────────

class ParamClosureThisDemo
{
    public function demo(): void
    {
        $router = new Scaffolding\ScaffoldingClosureThisRouter();

        // @param-closure-this overrides $this inside the closure to
        // Scaffolding\ScaffoldingClosureThisRoute instead of ParamClosureThisDemo.
        $router->group(function () {
            $this->middleware('auth');     // resolves Route::middleware()
            $this->prefix('/api');        // resolves Route::prefix()
        });

        // Chaining through the overridden $this
        $router->group(function () {
            $this->middleware('auth')->prefix('/v2');
        });

        // Nested closures: the innermost @param-closure-this wins. Note
        // that resolving the inner call's own $this receiver goes through
        // the outer override first (it is a Route, which is what declares
        // resource()).
        $router->group(function () {
            $this->resource('posts', function () {
                $this->only('index');     // resolves Resource::only()
            });
        });

        // @param-closure-this with $this as the type (declares the
        // closure's $this is the method's declaring class).
        $router->extend('redis', function () {
            $this->getDefaultDriver();    // resolves Router::getDefaultDriver()
        });

        // The tag names the base class, so an assertion is how a closure
        // body says which subclass it was actually bound to. Narrowing
        // refines the tag rather than being overruled by it.
        $router->apiGroup(function () {
            assert($this instanceof Scaffolding\ScaffoldingClosureThisApiRoute);
            $this->version('v2');         // resolves ApiRoute::version()
            $this->prefix('/api');        // still resolves Route::prefix()
        });

        // Macro-style registration: the closure is bound with the target
        // class as its scope, so self:: and static:: inside it refer to
        // Scaffolding\ScaffoldingMacroTarget, not ParamClosureThisDemo.
        Scaffolding\ScaffoldingMacroTarget::macro('renderTwice', function (): string {
            return self::make()->render()      // resolves MacroTarget::make()
                . static::make()->render();    // static:: works the same way
        });
    }
}


// ── Type Specificity in Virtual Property Merging ────────────────────────────

class TypeSpecificityDemo
{
    public function demo(): void
    {
        $cfg = new Scaffolding\ScaffoldingAppConfig();

        // Hover $cfg->locale — should show string (from native type hint),
        // not mixed (from the trait's @property tag).
        $cfg->locale;

        // Hover $cfg->timezone — should show string (from native type hint),
        // not mixed (from the trait's @property tag).
        $cfg->timezone;

        // Hover $cfg->retries — should show int (from native type hint),
        // not mixed (from the trait's @property tag).
        $cfg->retries;
    }
}


// ── Mixin Generic Substitution ──────────────────────────────────────────────

class MixinGenericDemo
{
    public function demo(): void
    {
        $line = new Scaffolding\ScaffoldingOrderLine();

        // @mixin Scaffolding\Builder<TRelatedModel> on Relation resolves TModel → Scaffolding\Product
        // through: BelongsTo @extends Relation<Scaffolding\Product> → @mixin Scaffolding\Builder<TRelatedModel>
        // → TRelatedModel=Scaffolding\Product → Scaffolding\Builder<Scaffolding\Product> → firstOrFail(): TModel=Scaffolding\Product
        $line->product()->firstOrFail()->getPrice();

        // Same resolution through find()
        $line->product()->find()->getSku();
    }
}


// ── Constant Type Inference ─────────────────────────────────────────────────
// Hover over $timeout, $name, $rate, $enabled, or $hosts to see the type
// inferred from the constant's initializer value.

class ConstantTypeDemo
{
    const TIMEOUT = 30;
    const NAME = 'app';
    const RATE = 3.14;
    const ENABLED = true;

    public function demo(): void
    {
        // Class constants without type hints — type inferred from value:
        $timeout = self::TIMEOUT;   // → int
        $name    = self::NAME;      // → string
        $rate    = self::RATE;      // → float
        $enabled = self::ENABLED;   // → bool

        // Global constants — type inferred from define()/const value:
        $hosts   = CT_ALLOWED_HOSTS;  // → array
        $version = Scaffolding\CT_APP_VERSION;    // → string
    }
}


// ── Attribute Completion ────────────────────────────────────────────────────
// Inside `#[…]`, completion only offers classes decorated with
// `#[\Attribute]`, filtered by the target of the declaration the
// attribute applies to.

class AttributeCompletionDemo
{
    public string $property;

    public function demo(): void
    {
        // Nothing to complete at runtime — this demo is about the
        // completion popup.  Open the class below and trigger
        // completion inside the `#[…]` brackets to see it in action.
    }
}



// ── Loop Array Build (variable-key assignment tracking) ─────────────────────

class LoopArrayBuildDemo
{
    /** @param list<Scaffolding\Pen> $pens */
    public function demo(array $pens): void
    {
        // Variable-key assignment inside a loop: `$arr[$var] = $value`
        // PHPantom tracks the RHS type as the array's element type.
        $indexed = [];
        foreach ($pens as $i => $pen) {
            $key = $pen->color();
            $indexed[$key] = $pen;
        }

        // Foreach over the built array resolves element members
        foreach ($indexed as $item) {
            $item->write();               // Scaffolding\Pen method via element type tracking
        }

        // Bracket access resolves element type
        $indexed['red']->color();         // Scaffolding\Pen method

        // Null-coalescing with guard clause
        $found = $indexed['blue'] ?? null;
        if ($found === null) { return; }
        $found->write();                  // narrowed to Scaffolding\Pen
    }
}

class ConditionalLoopShapeDemo
{
    /** @param list<Scaffolding\Pen> $pens */
    public function demo(array $pens): void
    {
        // Array built with variable keys inside a loop where the assignment
        // is inside a conditional branch (if/else). The shape type from
        // the array literal is preserved through foreach iteration.
        $grouped = [];
        foreach ($pens as $pen) {
            $key = $pen->color();
            if (array_key_exists($key, $grouped)) {
                $grouped[$key]['count']++;
            } else {
                $grouped[$key] = [
                    'tool'  => $pen,
                    'count' => 1,
                ];
            }
        }

        // Foreach over the built array resolves shape keys
        foreach ($grouped as $entry) {
            $entry['tool']->write();      // Scaffolding\Pen method via shape tracking
        }
    }
}


// ── Arrays the code proved have entries ─────────────────────────────────────
//
// A `null` seeded above a loop only survives the loop if the loop might run
// zero times.  `count($xs) > 0`, the fall-through of `count($xs) === 0`, and
// writing an element all say the array has entries, so a loop over it runs at
// least once and the sentinel is gone by the time the code below reads it.

class ProvenNonEmptyDemo
{
    /** @param list<Scaffolding\Pen> $pens */
    public function afterCountGuard(array $pens): Scaffolding\Pen
    {
        $last = null;
        if (count($pens) > 0) {
            foreach ($pens as $pen) {
                $last = $pen;
            }

            // Try: `$last->` — Scaffolding\Pen, not Scaffolding\Pen|null
            return $last;                         // Scaffolding\Pen
        }

        return new Scaffolding\Pen('black');
    }

    /** @param list<Scaffolding\Pen> $pens */
    public function afterEmptyGuard(array $pens): Scaffolding\Pen
    {
        if (count($pens) === 0) {
            throw new \RuntimeException('no pens');
        }

        // $pens is non-empty-list<Scaffolding\Pen> below the guard.
        $last = null;
        foreach ($pens as $pen) {
            $last = $pen;
        }

        return $last;                             // Scaffolding\Pen
    }

    public function afterElementWrite(Scaffolding\Pen $pen): Scaffolding\Pen
    {
        $collected = [];
        $collected[] = $pen;                      // non-empty-list<Scaffolding\Pen>

        $last = null;
        foreach ($collected as $item) {
            $last = $item;
        }

        return $last;                             // Scaffolding\Pen
    }

    /** A write on only one path gives the promise back where they join. */
    public function afterConditionalWrite(Scaffolding\Pen $pen, bool $keep): ?Scaffolding\Pen
    {
        $collected = [];
        if ($keep) {
            $collected[] = $pen;                  // array{}|non-empty-list<Scaffolding\Pen>
        }

        $last = null;
        foreach ($collected as $item) {
            $last = $item;
        }

        // Try: hover `$last` — Scaffolding\Pen|null, since the loop may not run
        return $last;                             // Scaffolding\Pen|null
    }
}


// ── Conditional Shape Key Completion ────────────────────────────────────────
// When an array shape gains a key inside an if-block, completion resolves
// through the union of shapes produced by branch merging.

class ConditionalShapeKeyDemo
{
    public function demo(?Scaffolding\Pen $pen): void
    {
        // Base shape with a known key
        $options = [
            'name' => 'default',
        ];

        // Conditionally add a key with an object value
        if ($pen !== null) {
            $options['tool'] = $pen;
        }

        // After the if-block, $options is a union of shapes:
        //   array{name: string} | array{name: string, tool: Scaffolding\Pen}
        // Completion on the conditionally-added key resolves to Scaffolding\Pen.
        $options['tool']->write();        // Scaffolding\Pen method via conditional shape union
    }
}


// ── Untyped Property Inference ──────────────────────────────────────────────
// Properties without type declarations have their types inferred from
// constructor assignments (`$this->prop = new Foo()`) and promoted
// parameter defaults (`private $prop = new Foo()`). Trigger completion
// after `->` on the property to see methods from the inferred type.

class UntypedPropertyInferenceDemo
{
    private $repository;
    private $logger;

    public function __construct(
        private $defaultRepo = new Scaffolding\ScaffoldingUntypedRepo(),
    ) {
        $this->repository = new Scaffolding\ScaffoldingUntypedRepo();
        $this->logger = new Scaffolding\ScaffoldingUntypedLogger();
    }

    public function demo(): void
    {
        // Constructor body assignment: $this->repository = new Scaffolding\ScaffoldingUntypedRepo()
        $this->repository->findById(1);       // resolves Scaffolding\ScaffoldingUntypedRepo::findById()

        // Constructor body assignment: $this->logger = new Scaffolding\ScaffoldingUntypedLogger()
        $this->logger->info('hello');         // resolves Scaffolding\ScaffoldingUntypedLogger::info()

        // Promoted parameter default: private $defaultRepo = new Scaffolding\ScaffoldingUntypedRepo()
        $this->defaultRepo->findById(42);     // resolves Scaffolding\ScaffoldingUntypedRepo::findById()
    }
}


// ── Deep Variable Chain ─────────────────────────────────────────────────────
// The variable resolver walks function bodies top-to-bottom in a single pass.
// Assignment chains of any depth resolve without recursion or depth limits.
// Place the cursor after `->` on any variable below to see completions from
// the correct class, regardless of how many intermediate assignments there are.

class DeepVariableChainDemo
{
    public function demo(): void
    {
        // 5-level chain: each variable is assigned from a method/property on the previous.
        $brush = new Scaffolding\Brush();
        $canvas = $brush->getCanvas();
        $easel = $canvas->easel;
        $material = $easel->material;         // string from Scaffolding\Easel::$material
        $back = $canvas->getBrush();
        $back->stroke();                      // Scaffolding\Brush::stroke() — full round-trip

        // Reassignment chains: the resolver picks the most recent assignment.
        $tool = new Scaffolding\Pen();
        $tool->write();                       // Scaffolding\Pen::write()
        $tool = new Scaffolding\Pencil();
        $tool->sketch();                      // Scaffolding\Pencil::sketch() — Scaffolding\Pen::write() is gone
        $tool = new Scaffolding\Marker();
        $tool->highlight();                   // Scaffolding\Marker::highlight()
    }
}


// ── Closure Scope Inference ─────────────────────────────────────────────────
// Closures capture variables from the enclosing scope via `use()`. Arrow
// functions inherit the enclosing scope automatically. Untyped closure
// parameters are inferred from the callable signature of the enclosing call.

class ClosureScopeInferenceDemo
{
    /** @param list<Scaffolding\Pen> $pens */
    public function demo(array $pens): void
    {
        // Closure captures $pens via use() and iterates over it.
        $worker = function () use ($pens): void {
            foreach ($pens as $pen) {
                $pen->write();                // Scaffolding\Pen from captured $pens
            }
        };

        // Arrow function inherits enclosing scope automatically.
        $brush = new Scaffolding\Brush();
        $sized = fn() => $brush->setSize('large');

        // Variables survive past closure arguments in chained calls.
        $product = new Scaffolding\Pen();
        $items = [1, 2, 3];
        array_map(function (int $i) { return $i * 2; }, $items);
        $product->write();                    // Scaffolding\Pen — not lost after the closure
    }
}

// ── Body Return Type Inference ──────────────────────────────────────────────
// When a method has no declared return type and no @return docblock,
// PHPantom infers the type by scanning the method body for return statements.

class BodyReturnTypeDemo
{
    public function demo(): void
    {
        $factory = new Scaffolding\ScaffoldingUntypedFactory();

        // Single return: `return new Scaffolding\Pen()` → Scaffolding\Pen
        $pen = $factory->createPen();
        $pen->write();

        // A static call with no declared return type reads the body the
        // same way an instance call does.
        $staticPen = Scaffolding\ScaffoldingUntypedFactory::createPenStatic();
        $staticPen->write();

        // A declared `@return mixed` carries no information — every type
        // satisfies it — so the body is still read for a real answer.
        $mixedPen = $factory->createPenMixed();
        $mixedPen->write();

        // Multiple returns: union of `new Scaffolding\Pen()` and `new Scaffolding\Pencil()`
        $tool = $factory->createTool(true);
        $tool->write();                           // shared by Scaffolding\Pen (also Scaffolding\Pencil via sketch)

        // No return statements → void (no completions)
        $factory->setup();

        $pencils = $factory->getPencils();
        foreach ($pencils as $pencil) {
            $pencil->sketch();
        }
    }
}

// ── Global Keyword ─────────────────────────────────────────────────────────

$globalPen = new Scaffolding\Pen();

function globalKeywordDemo(): void {
    global $globalPen;
    $globalPen->write();                  // Scaffolding\Pen — resolved from top-level scope via `global`
}

// ── Method-Tag Template ─────────────────────────────────────────────────────

class MethodTagTemplateDemo
{
    public function demo(): void
    {
        // @method tags with <T of Bound> template params resolve at call sites.
        $registry = new Scaffolding\ScaffoldingMethodTagTemplate();

        // TVal inferred from argument type
        $pen = new Scaffolding\Pen('demo');
        $result = $registry->get($pen);
        $result->write();                 // TVal = Scaffolding\Pen

        // Inline chain
        $registry->get(new Scaffolding\Pencil())->sketch(); // TVal = Scaffolding\Pencil
    }
}


// ── Multi-Line @method / @property Tags ─────────────────────────────────────

class MultiLineDocblockTagDemo
{
    public function demo(): void
    {
        // A @method or @property tag whose type wraps onto continuation lines
        // resolves like its single-line form, and every name written inside the
        // tag stays navigable: try go-to-definition on `Scaffolding\FluentCollection`,
        // `Scaffolding\Pen`, `fetchAll` or `$penHolder` in the docblock of
        // Scaffolding\ScaffoldingMultiLineTags.
        $shed = new Scaffolding\ScaffoldingMultiLineTags();

        $shed->fetchAll('black')->first()->write();  // TValue resolves to Scaffolding\Pen
        $shed->penHolder->first()->write();          // same through @property
    }
}

/**
 * Convert to arrow function — place cursor on a single-expression closure
 * and trigger code actions to see "Convert to arrow function".
 */
class ConvertToArrowFunctionDemo
{
    public function demo(): void
    {
        // Try: place cursor on `function` and use code action
        $double = function(int $x): int { return $x * 2; };

        // Static closure with use clause (by-value)
        $base = 10;
        $add = static function(int $x) use ($base) { return $x + $base; };

        // Passes as callback — trigger inside the closure
        $result = array_map(function(string $s) { return strtoupper($s); }, ['a', 'b']);
    }
}

class ConvertToClosureDemo
{
    public function demo(): void
    {
        // Try: place cursor on `fn` and use code action "Convert to closure"
        $double = fn(int $x): int => $x * 2;

        // Arrow with captured outer variable — converted closure gets use()
        $base = 10;
        $add = fn(int $x) => $x + $base;

        // Static arrow function
        $staticFn = static fn(string $s): string => strtoupper($s);

        // Arrow as callback — trigger inside the arrow function
        $result = array_map(fn(string $s) => strtoupper($s), ['a', 'b']);

        // Multiple captured variables
        $prefix = 'hello';
        $suffix = 'world';
        $greet = fn(string $sep) => $prefix . $sep . $suffix;
    }
}

// ── @phpstan-require-extends: base members on $this in a trait ───────────────

/**
 * A trait annotated with `@phpstan-require-extends` guarantees that every
 * class using it extends the named base class, so `$this` inside the trait
 * can access that base class's members even though the trait analyzed
 * standalone does not declare them.
 *
 * @phpstan-require-extends Scaffolding\RequireExtendsTestCase
 */
trait MocksServiceDemo
{
    public function mockPath(): string
    {
        $mock = $this->makeMock();                // Scaffolding\RequireExtendsTestCase::makeMock() → Scaffolding\Mock
        return $mock->path();                     // Scaffolding\Mock::path() → string
    }
}

// ── ReflectionClass<T> instantiation ────────────────────────────────────────
// `new ReflectionClass($classString)` binds the reflected type from a
// `class-string<T>`.  `newInstance()` returns `T` and `newInstanceArgs()`
// returns `T|null`, even though the constructor's native hint is the broad
// `object|string`.  `new ReflectionObject($instance)` binds the same way from
// the instance it is handed.

class ReflectionInstantiationDemo
{
    /**
     * @param class-string<Scaffolding\ReflectedWidget> $class
     */
    public function build(string $class): Scaffolding\ReflectedWidget
    {
        // Fully-qualified `\ReflectionClass` so the unused-import demo in
        // code_actions.php keeps its `use ReflectionClass;` dimmed.
        $reflection = new \ReflectionClass($class);

        // newInstance() → Scaffolding\ReflectedWidget (not class-string<Scaffolding\ReflectedWidget>)
        return $reflection->newInstance();
    }

    /**
     * Reading a property back through reflection.
     *
     * `getProperty()` is declared to return a bare `ReflectionProperty` and
     * `getValue()` a bare `mixed`, but the reflected class and the property
     * name are both known here, so the read types as the declaration does.
     */
    public function reflectedWidget(Scaffolding\ReflectedHolder $holder): ?Scaffolding\ReflectedWidget
    {
        $reflection = new \ReflectionObject($holder); // ReflectionObject<Scaffolding\ReflectedHolder>
        $property = $reflection->getProperty('widget');

        // Try: `$property->getValue($holder)->` — Scaffolding\ReflectedWidget
        // members (label()), because the read is not `mixed`
        return $property->getValue($holder);          // ?Scaffolding\ReflectedWidget
    }

    /**
     * The same read, written without the `ReflectionClass` step.
     *
     * `new \ReflectionProperty($class, $name)` and
     * `(new \ReflectionClass($class))->getProperty($name)` build the same
     * value, so the direct spelling carries the class and the name too.
     */
    public function directWidget(Scaffolding\ReflectedHolder $holder): ?Scaffolding\ReflectedWidget
    {
        // ReflectionProperty<Demo\Scaffolding\ReflectedHolder, 'widget'>
        $property = new \ReflectionProperty(Scaffolding\ReflectedHolder::class, 'widget');

        // Try: `$property->getValue($holder)->` — Scaffolding\ReflectedWidget
        // members (label()), the same as the getProperty() spelling above
        return $property->getValue($holder);          // ?Scaffolding\ReflectedWidget
    }

    /**
     * A reflection-based accessor, which is how the read above is usually
     * written: the object and the property name both arrive as arguments,
     * so nothing the signature could say would be true of every call.  The
     * `@return mixed` is the honest declaration, and the real type is read
     * off the body once a call site has decided the arguments.
     *
     * @return mixed Value of $object->$property
     */
    public static function fetchProperty($object, string $name)
    {
        $property = self::propertyOf(new \ReflectionObject($object), $name);

        return $property->getValue($object);
    }

    /** The declared `\ReflectionProperty` keeps the class and name it bound. */
    private static function propertyOf(\ReflectionClass $reflection, string $name): \ReflectionProperty
    {
        return $reflection->getProperty($name);
    }

    public function accessedWidget(Scaffolding\ReflectedHolder $holder): ?Scaffolding\ReflectedWidget
    {
        // Try: `self::fetchProperty($holder, 'widget')->` —
        // Scaffolding\ReflectedWidget members, from the two arguments alone
        return self::fetchProperty($holder, 'widget'); // ?Scaffolding\ReflectedWidget
    }
}

// ── Lazy initialisation inside a guarded `if` ───────────────────────────────
// A property assigned inside a guard keeps the assigned type once the block
// closes.  Both ways out give the same type: the then-branch assigns it, and
// the implicit else path is the negation of the condition.

class LazyInitNarrowingDemo
{
    private ?Scaffolding\Pen $marker = null;

    public function marker(): Scaffolding\Marker
    {
        if (!$this->marker instanceof Scaffolding\Marker) {
            $this->marker = new Scaffolding\Marker();
        }

        // Try: `$this->marker->` — Scaffolding\Marker members (highlight(), write(), …)
        return $this->marker;                     // Scaffolding\Marker, not ?Scaffolding\Pen
    }
}

// ── Builtins whose failure branch is conventionally never checked ───────────
//
// `tempnam()` is declared `string|false`, and so are a couple of hundred other
// builtins whose `false` only appears when something has gone wrong that the
// caller could not do anything about locally.  Real code passes the result
// straight on, and PHPantom follows PHPStan in not reporting that.
//
// The union itself is untouched: hover on `$tmp` below still reads
// `string|false`, and a caller that does check the branch still narrows
// through it.  Only the argument/property/return checks stop insisting on it.

class BenevolentBuiltinDemo
{
    /** No diagnostic: tempnam()'s `false` is not worth reporting. */
    public function writeReport(string $body): string
    {
        $tmp = tempnam(sys_get_temp_dir(), 'report');   // string|false
        file_put_contents($tmp, $body);                 // $filename expects string

        return $tmp;
    }

    /** Checking the branch still narrows it away. */
    public function writeCheckedReport(string $body): ?string
    {
        $tmp = tempnam(sys_get_temp_dir(), 'report');
        if ($tmp === false) {
            return null;                                // $tmp is false here
        }
        file_put_contents($tmp, $body);                 // $tmp is string here

        return $tmp;
    }
}

// The leniency is tied to the listed builtins, not to `|false` at large.
// `strpos()` is deliberately not on the list: its `false` means "not found",
// which is an answer the caller is meant to read, so
// `takesInt(strpos($h, $n))` is still reported.

// ── Magic constants ────────────────────────────────────────────────────────
//
// PHP's magic constants are values like any other, and each carries its own
// type: `__LINE__` is an int, the rest are strings.  `__CLASS__` keeps the
// class it names, the way `Foo::class` does, so the name it produces still
// satisfies a `class-string` parameter.

class MagicConstantDemo
{
    public function lineOffset(): int
    {
        // Try: hover `__LINE__` — int, so the sum below is an int too
        return __LINE__ + 3;                        // int, not int|float
    }

    public function describe(): string
    {
        $file = __FILE__;                           // string
        $method = __METHOD__;                       // string

        // Try: `$method->` — nothing, a string has no members
        return $method . ' in ' . basename($file);
    }

    /** @return class-string */
    public function ownName(): string
    {
        return __CLASS__;                           // class-string<MagicConstantDemo>
    }
}

// ── Proofs the condition never states outright ──────────────────────────────
//
// A guard records what it proved under the spelling it tested, and three
// idioms read that proof back through something the condition never names:
// a disjunction whose surviving leg is picked further down, the identical
// condition tested a second time, and a closure that captures a value a
// guard above it already narrowed.

class ReconstructedProofDemo
{
    /**
     * The `||` proves only that one leg held.  Ruling the `=== null` leg
     * out leaves the other one, and with it the `is_string` it carried.
     */
    public function keyName(Scaffolding\ScaffoldingLoopNode $node): ?string
    {
        if (
            is_string($node->valueVar->name)
            && (
                $node->keyVar === null
                || ($node->keyVar instanceof Scaffolding\ScaffoldingNameNode && is_string($node->keyVar->name))
            )
        ) {
            // Try: hover `$node->keyVar->name` — string, not string|ScaffoldingNameNode
            return $node->keyVar instanceof Scaffolding\ScaffoldingNameNode
                ? $node->keyVar->name
                : null;                                 // ?string
        }

        return null;
    }

    /**
     * The second `count($args) > 0` re-establishes what the first one did,
     * and that branch is the only thing that fills `$acceptor`.
     *
     * @param string[] $args
     */
    public function describeArguments(array $args): string
    {
        $acceptor = null;
        if (count($args) > 0) {
            $acceptor = Scaffolding\scaffoldingSelectAcceptor($args);
        }

        if (count($args) > 0) {
            // Try: `$acceptor->` — ScaffoldingArgumentAcceptor members, no null
            return $acceptor->describe($args);      // Scaffolding\ScaffoldingArgumentAcceptor
        }

        return '';
    }

    /**
     * `use ($holder)` captures the value `$holder->label` is read through,
     * so the guard above the closure still holds inside its body.
     */
    public function labelPrinter(Scaffolding\ScaffoldingOptionalLabel $holder): \Closure
    {
        if ($holder->label === null) {
            return static fn (): string => '';
        }

        return static function () use ($holder): string {
            // Try: hover `$holder->label` — string, not ?string
            return strtoupper($holder->label);      // string
        };
    }

    /**
     * A plain boolean is a condition of its own.  The branch it guards is
     * the only thing that fills `$acceptor`, so re-testing the flag is
     * testing whether that branch ran.
     *
     * @param string[] $args
     */
    public function describeWhenFlagged(array $args, bool $wanted): string
    {
        $acceptor = null;
        if ($wanted) {
            $acceptor = Scaffolding\scaffoldingSelectAcceptor($args);
        }

        if ($wanted) {
            // Try: `$acceptor->` — ScaffoldingArgumentAcceptor members, no null
            return $acceptor->describe($args);      // Scaffolding\ScaffoldingArgumentAcceptor
        }

        // Try: hover `$wanted` — false, since the branch above was skipped
        return $wanted ? 'unreachable' : '';        // string
    }

    /**
     * The path that failed `instanceof ScaffoldingQualifiedName` keeps the
     * parent class, which spans the one the check named, so the types the
     * two paths leave behind say nothing about which of them ran.  What
     * does is the class the failing check ruled out.
     */
    public function qualifiedLabel(Scaffolding\ScaffoldingNameNode $node): string
    {
        $acceptor = null;
        if ($node instanceof Scaffolding\ScaffoldingQualifiedName) {
            $acceptor = Scaffolding\scaffoldingSelectAcceptor(['q']);
        }

        // Try: `$acceptor->` — ScaffoldingArgumentAcceptor members, no null
        return $node instanceof Scaffolding\ScaffoldingQualifiedName
            ? $acceptor->describe(['q'])            // Scaffolding\ScaffoldingArgumentAcceptor
            : '';
    }

    /**
     * Past the guard at least one of the two flags held, so the arm that
     * knows `$firstIsQualified` is false knows `$secondIsQualified` is
     * not, and with it what the check behind that flag proved about
     * `$second`.  Neither arm names the flag it relies on.
     */
    public function eitherPrefix(
        Scaffolding\ScaffoldingNameNode $first,
        Scaffolding\ScaffoldingNameNode $second
    ): string {
        $firstIsQualified = $first instanceof Scaffolding\ScaffoldingQualifiedName;
        $secondIsQualified = $second instanceof Scaffolding\ScaffoldingQualifiedName;
        if (!$firstIsQualified && !$secondIsQualified) {
            return '';
        }

        // Try: `$qualified->` — ScaffoldingQualifiedName members from both arms
        $qualified = $firstIsQualified ? $first : $second;

        return $qualified->namespacePrefix;         // string
    }
}
