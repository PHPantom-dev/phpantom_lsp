<?php

/**
 * PHP Showcase — Navigation
 *
 * Go-to-definition, go-to-type-definition, go-to-implementation, and the
 * type hierarchy. Ctrl+Click a name, or use "Go to Type Definition" to
 * jump to the class declaration of a variable's resolved type.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

// Ctrl+Click the name below to jump to the function it imports — a
// `use function` (and `use const`) import is the symbol it names, so
// find-references and rename reach it like any other mention.
use function Demo\Scaffolding\coverageTaxRate; // needed for the bare `::coverageTaxRate` @covers demo below, though never named directly

// ── Go-to-Definition ────────────────────────────────────────────────────────
// All jump targets are defined right after the demo so Ctrl+Click lands
// within a few lines, making it easy to verify the feature works.
//
// Member names deliberately collide with names elsewhere in the file
// (label, format, CONNECTION, $defaultRole) so a wrong-target bug
// would land on the wrong label() or CONNECTION instead of silently passing.

class GoToDefinitionDemo
{
    public function demo(): void
    {
        // Ctrl+Click on any symbol to jump to its definition
        $target = new GtdTarget();
        $target->label();                         // Ctrl+Click → GtdTarget::label() (not Scaffolding\Pen::label)
        $target->format();                        // Ctrl+Click → GtdTarget::format() (not Scaffolding\User::format)
        GtdTarget::FORMAT;                        // Ctrl+Click → class constant (not Scaffolding\Renderable::format)
        GtdParent::CONNECTION;                    // Ctrl+Click → GtdParent (not Scaffolding\Model::CONNECTION)
        GtdTarget::$defaultRole;                  // Ctrl+Click → GtdTarget (not Scaffolding\User::$defaultRole)

        $helper = gtdHelper();
        echo $helper;                             // Ctrl+Click on $helper → jumps to assignment

        define('APP_VERSION', '1.0.0');
        echo APP_VERSION;                         // BUG: Ctrl+Click should jump to define() above
    }
}

class GtdParent { public const string CONNECTION = 'gtd'; }
class GtdTarget extends GtdParent
{
    public static string $defaultRole = 'gtd';
    public const string FORMAT = 'gtd';
    public function label(): string { return 'gtd'; }
    public function format(): string { return 'gtd'; }
}
function gtdHelper(): GtdTarget { return new GtdTarget(); }


// ── Property Hook Bodies (PHP 8.4) ──────────────────────────────────────────
// A `get`/`set` hook body reads like any other body: Ctrl+Click reaches
// what is written inside one, `$this` is the class that declares the hook,
// a local keeps its type from one statement to the next, and the value a
// `set` hook receives carries the property's type whether it is spelled
// out or left implicit.

class HookedGtdDemo
{
    public GtdTarget $source;
    public string $lastSeen = '';

    // Expression-bodied hook
    public string $formatted {
        get => $this->source->format();       // Ctrl+Click → GtdTarget::format()
    }

    // Block-bodied hook — $target is a local of the hook body
    public string $labelled {
        get {
            $target = $this->source;
            return $target->label();          // Ctrl+Click → GtdTarget::label()
        }
    }

    // A declared `set` value
    public GtdTarget $declaredWrite {
        get => $this->source;
        set(GtdTarget $value) {
            $this->lastSeen = $value->format();  // Ctrl+Click → GtdTarget::format()
        }
    }

    // An implicit `$value`, typed as the property
    public GtdTarget $implicitWrite {
        get => $this->source;
        set {
            $this->lastSeen = $value->label();   // Ctrl+Click → GtdTarget::label()
        }
    }

    public function __construct()
    {
        $this->source = new GtdTarget();
    }
}

// A hook on a constructor-promoted property, and a hook that calls the one
// it overrides through PHP 8.4's `parent::$prop::get()` spelling.
//
// A promoted hooked property needs a `set` hook: promotion assigns the
// argument, and a get-only virtual property is read-only.
class PromotedHookGtdDemo extends HookedGtdDemo
{
    public string $seen = '';

    public function __construct(
        public GtdTarget $origin,
        public GtdTarget $promoted {
            get => $this->origin;
            set {
                $this->seen = $value->label();  // Ctrl+Click → GtdTarget::label()
            }
        },
    ) {
        parent::__construct();
    }

    public string $formatted {
        get => parent::$formatted::get();       // Ctrl+Click $formatted → HookedGtdDemo::$formatted
    }
}


// ── Type Hint Go-to-Definition ──────────────────────────────────────────────
// Ctrl+Click on class names in type hints, return types, catch blocks,
// and docblock annotations to jump to their definitions.
// All referenced types are defined right after the demo so the jump is short.
//
// Support classes have format()/label() methods that collide with names
// elsewhere — if GTD resolves the wrong class, you land on the wrong one.

class TypeHintGtdDemo
{
    public function demo(): void
    {
        // Catch block exception types — Ctrl+Click GtdNotFoundException or GtdAccessException
        try {
            $this->paramTypes(new GtdAlpha());
        } catch (GtdNotFoundException|GtdAccessException) {}
    }

    public function paramTypes(GtdAlpha $item): GtdAlpha { return $item; }                             // Ctrl+Click GtdAlpha
    public function unionTypes(GtdAlpha|GtdBeta $item): GtdAlpha|GtdBeta { return $item; }             // Ctrl+Click either
    public function intersectionTypes(GtdShape&GtdColor $item): GtdShape&GtdColor { return $item; }    // Ctrl+Click either
    public function returnType(): GtdResult { return new GtdResult(); }                                // Ctrl+Click GtdResult

    /**
     * @param list<GtdAlpha> $items                Ctrl+Click GtdAlpha
     * @return GtdResult                           Ctrl+Click GtdResult
     * @throws GtdNotFoundException                Ctrl+Click GtdNotFoundException
     */
    public function docblockTypes($items) { return new GtdResult(); }

    /**
     * Callable types in docblocks. Ctrl+Click on any class name inside the
     * callable signature to jump to its definition. Hover shows the class
     * info instead of treating the whole callable as one token.
     *
     * @param \Closure(GtdAlpha): GtdResult $transform      Ctrl+Click GtdAlpha or GtdResult
     * @param callable(GtdAlpha, GtdBeta): GtdResult $merge Ctrl+Click any of the three
     * @return callable(): GtdResult                         Ctrl+Click GtdResult
     */
    public function callableDocblockTypes($transform, $merge) { return $merge; }
}

class GtdAlpha { public function label(): string { return 'alpha'; } }
class GtdBeta { public function label(): string { return 'beta'; } }
interface GtdShape { public function format(): string; }
interface GtdColor { public function format(): string; }
class GtdResult { public function label(): string { return 'ok'; } }
class GtdNotFoundException extends \RuntimeException {}
class GtdAccessException extends \RuntimeException {}


// ── Go-to-Type-Definition ───────────────────────────────────────────────────
// "Go to Type Definition" jumps to the *type's* class declaration, not the
// variable's definition site. Compare with regular Go-to-Definition:
//   • Go-to-Definition on $user   → jumps to the $user = ... assignment
//   • Go-to-Type-Definition on $user → jumps to class Scaffolding\User { ... }
//
// Try: place the cursor on $target, $result, or $pet below and invoke
// "Go to Type Definition" (often bound to a secondary shortcut or
// right-click menu). Union types produce a peek list with all classes.

class GoToTypeDefinitionDemo
{
    public function demo(): void
    {
        $target = new GtdTarget();
        $target;                                  // Type Definition → GtdTarget

        $result = $this->getResult();
        $result;                                  // Type Definition → GtdResult

        $pet = $this->getPet();
        $pet;                                     // Type Definition → GtdAlpha | GtdBeta (two locations)

        $this;                                    // Type Definition → GoToTypeDefinitionDemo
    }

    public function getResult(): GtdResult { return new GtdResult(); }

    /** @return GtdAlpha|GtdBeta */
    public function getPet(): GtdAlpha|GtdBeta { return new GtdAlpha(); }
}


// ── Go-to-Implementation ────────────────────────────────────────────────────
// All implementors are defined right after the demo so "Go to Implementations"
// lands within a few lines.
//
// The interface method is format() — same name as Scaffolding\Renderable::format(),
// Scaffolding\User::format(), Scaffolding\Ingredient::format(). A resolver bug would jump to one
// of those instead of the local implementor.

class GoToImplementationDemo
{
    // Right-click → "Go to Implementations" on GtdPrintable
    // to jump to GtdPlainPrinter and GtdHtmlPrinter below.
    // Try: Go-to-Implementation on "format" → format() in each implementor
    public function demo(GtdPrintable $printer): string
    {
        return $printer->format();
    }
}

interface GtdPrintable { public function format(): string; }
class GtdPlainPrinter implements GtdPrintable { public function format(): string { return 'plain'; } }
class GtdHtmlPrinter implements GtdPrintable { public function format(): string { return '<b>html</b>'; } }


// ── Reverse Go-to-Implementation ────────────────────────────────────────────
// Go-to-Implementation also works in reverse: from a concrete method back to
// the interface or abstract method it satisfies.

class ReverseImplementationDemo implements GtdPrintable
{
    // Try: Go-to-Implementation on "format" below → jumps to
    // GtdPrintable::format() (the interface prototype).
    public function format(): string
    {
        return 'reverse';
    }
}


// ── Type Hierarchy ──────────────────────────────────────────────────────────
// Right-click a class/interface name → "Show Type Hierarchy" to see its
// supertypes (parent class, implemented interfaces) and subtypes (classes
// that extend or implement it).
//
// Try on GtdPrintable: supertypes → (none), subtypes → GtdPlainPrinter, GtdHtmlPrinter, ReverseImplementationDemo
// Try on ReverseImplementationDemo: supertypes → GtdPrintable, subtypes → (none)
// Try on Scaffolding\User: supertypes → Scaffolding\Model, Scaffolding\Renderable, subtypes → Scaffolding\AdminUser
// Try on Scaffolding\Model: supertypes → (none), subtypes → Scaffolding\User, ClassFilteringDemo (completion.php),
// HoverOriginsDemo (hover.php)


// ── PHPUnit coverage metadata (go-to-definition) ────────────────────────────
// `@covers` and `@uses` name the code a test exercises.  Ctrl+Click any target
// to jump to it.  `@coversDefaultClass` gives a bare `::member` its subject,
// and it reaches the docblocks of the class's methods as well.
//
// PHPUnit 10 replaced these annotations with attributes.  Those navigate the
// same way once PHPUnit is installed, including the targets it spells as
// strings: `#[CoversClass(Scaffolding\CoverageCalculator::class)]`,
// `#[CoversMethod(Scaffolding\CoverageCalculator::class, 'add')]`, and
// `#[CoversFunction('Demo\\Scaffolding\\coverageTaxRate')]`.

/**
 * @coversDefaultClass \Demo\Scaffolding\CoverageCalculator
 *
 * @covers ::add                          Scaffolding\CoverageCalculator::add()
 * @uses \Demo\Scaffolding\CoverageLedger Scaffolding\CoverageLedger
 */
class CoverageDefaultClassDemo
{
    /**
     * @covers ::subtract                 Scaffolding\CoverageCalculator::subtract()
     */
    public function demo(): void
    {
    }
}

/**
 * Without a default class in scope, `::name` names a global function.
 *
 * @covers ::coverageTaxRate                         Scaffolding\coverageTaxRate()
 * @covers \Demo\Scaffolding\CoverageCalculator::add  Scaffolding\CoverageCalculator::add()
 */
class CoverageFunctionTargetDemo
{
}


// ── PHPUnit data providers and dependencies (go-to-definition) ─────────────
// `@dataProvider` and `@depends` name a method by string.  Ctrl+Click the
// name to jump to it; renaming the method updates the tag too.  The PHPUnit
// 10 attributes navigate the same way once PHPUnit is installed:
// `#[DataProvider('additions')]`, `#[Depends('testCreatesALedger')]`, and
// `#[DataProviderExternal(Fixtures::class, 'additions')]` for a provider
// that lives in another class.

class DataProviderDemo
{
    /**
     * @dataProvider additions                DataProviderDemo::additions()
     */
    public function testAdds(int $a, int $b, int $sum): void
    {
    }

    public function testCreatesALedger(): Scaffolding\CoverageLedger
    {
        return new Scaffolding\CoverageLedger();
    }

    /**
     * @depends clone testCreatesALedger      DataProviderDemo::testCreatesALedger()
     */
    public function testUsesTheLedger(Scaffolding\CoverageLedger $ledger): void
    {
    }

    /** @return iterable<string, array{int, int, int}> */
    public static function additions(): iterable
    {
        yield 'small' => [1, 2, 3];
    }
}
