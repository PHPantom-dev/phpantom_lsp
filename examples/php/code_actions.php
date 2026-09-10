<?php

/**
 * PHP Showcase — Code Actions
 *
 * The quick fixes and refactorings offered on the lightbulb: importing and
 * tidying `use` statements, generating docblocks, constructors, accessors
 * and property hooks, changing visibility, extracting a function, promoting
 * a constructor parameter, and simplifying an expression.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

use Closure;
use Demo\Scaffolding;

// ── Code Action: Import Class ───────────────────────────────────────────────
// Place cursor on `MutateArrayInsertSpec` and press Ctrl+. (or Cmd+. on Mac)
// to see "Import `Couchbase\MutateArrayInsertSpec`" in the quick-fix menu.
// Accepting inserts a `use Couchbase\MutateArrayInsertSpec;` at the top.
//
// Because this file has two unresolved names, the quick-fix menu also shows
// "Import all missing classes" which imports both at once.

class ImportClassDemo
{
    public function demo(): void
    {
        // Ctrl+. on `MutateArrayInsertSpec` → offers to import
        $spec = new MutateArrayInsertSpec('path', ['value']);

        // Ctrl+. on `Cluster` → offers to import Couchbase\Cluster
        Cluster::connect('couchbase://localhost');
    }
}


// ── Code Action: Import Qualified Name ─────────────────────────────────────
// Place the cursor on either qualified name and trigger "Code Action".
// The refactoring adds the appropriate import and replaces every equivalent
// usage in this file with its short name. The companion action alongside it,
// "Import all qualified symbols and shorten usages", does the same for every
// qualified class, function, and constant in this namespace at once.

class ImportQualifiedNameDemo
{
    public function demo(): Scaffolding\Pen
    {
        return \Demo\Scaffolding\makePen();
    }
}


// ── Code Action: Remove Unused Import ───────────────────────────────────────
// The `use ReflectionClass;` below is unused — it appears dimmed in the editor.
// Place cursor on it and press Ctrl+. → "Remove unused import 'ReflectionClass'"

use ReflectionClass;

class RemoveUnusedImportDemo
{
    public function demo(): void
    {
        // ReflectionClass is deliberately NOT used here so its import stays dimmed.
        // Ctrl+. on the dimmed `use ReflectionClass;` above → remove it.
        $x = 42;
    }
}


// ── Code Action: Sort Use Statements ────────────────────────────────────────
// The two imports below are out of alphabetical order. Place cursor on either
// one and press Ctrl+. → "Sort use statements" reorders them so `ArrayObject`
// comes before `SplStack`.

use SplStack;
use ArrayObject;

class SortUseStatementsDemo
{
    public function demo(): void
    {
        $stack = new SplStack();
        $wrapped = new ArrayObject([1, 2, 3]);
        $stack->push($wrapped);
    }
}


// ── PHPDoc Block Generation ─────────────────────────────────────────────────
// Typing `/**` above a declaration generates a docblock skeleton.  Tags are
// only emitted when the native type hint needs enrichment: missing types get
// @param/${mixed}, bare `array` gets a placeholder, and classes with @template
// parameters get generic type tab stops (e.g. Collection<TKey, TValue>).
// Fully-typed scalar params/return types are skipped.  Properties and
// constants always get @var.  Uncaught exceptions always get @throws.
// No special treatment for overrides.

class PhpDocGenerationDemo extends Scaffolding\ScaffoldingException
{
    public const int MAX_ITEMS = 100;
    const LABEL = 'demo';

    public string $title = '';
    public $description;

    public function demo($data, array $items, Closure $handler, callable $fallback, Scaffolding\TypedCollection $primary, string $boring, Scaffolding\TypedCollection $secondary): array
    {
        try {
            throw new Scaffolding\ValidationException('Invalid id');
        } catch (Scaffolding\ValidationException $e) {
            // Caught — should NOT appear in @throws.
        }

        /** @throws Scaffolding\NotFoundException */
        Scaffolding\getUnknownValue();

        $this->throwsException();

        return [];
    }
}


// Class-level @extends with template tab stops.  The parent Scaffolding\TypedCollection
// has @template TKey and @template TValue, so typing `/**` above this class
// generates `@extends Scaffolding\TypedCollection<TKey, TValue>` with tab stops.
// Try: type `/**` above this class.
class DocGenExtendsDemo extends Scaffolding\TypedCollection
{
    public function customMethod(): void {}
}


// ── Implement Missing Methods (Code Action) ─────────────────────────────────
// Uncomment the class below, place the cursor inside it, and trigger
// "Quick Fix" or "Code Action" to see "Implement 3 missing methods".
// The generated stubs include correct visibility, parameter types, defaults,
// and return types.  Re-comment when done (PHP fatals on unimplemented
// abstract methods).

// class ImplementMethodsDemo extends Scaffolding\ScaffoldingAbstractShape implements Scaffolding\ScaffoldingDrawable
// {
// }


// ── Generate Constructor (Code Action) ──────────────────────────────────────
// Place the cursor inside the class below and trigger "Code Action" to see
// "Generate constructor".  The generated constructor includes a parameter
// and assignment for each non-static property.  Readonly properties are
// included because they must be initialized in the constructor.  Default
// values are carried over and required parameters are placed before
// optional ones.

class GenerateConstructorDemo
{
    public int $age;
    public string $name;
    public string $status = 'active';
    public ?string $email;
    public readonly string $id;
    public static int $instanceCount;     // excluded (static)
}


// ── Generate Getter/Setter (Code Action) ────────────────────────────────────
// Place the cursor on a property declaration below and trigger "Code Action"
// to see "Generate getter", "Generate setter", and "Generate getter and
// setter".  Bool properties use an `is` prefix (`isActive()`).  Readonly
// properties only offer a getter.  Static properties generate static
// methods.  If a getter or setter already exists, the corresponding action
// is suppressed.

class GenerateGetterSetterDemo
{
    private string $name;
    private bool $active;
    public readonly int $id;
    private static int $count;
    /** @var list<string> */
    public $tags;
}


// ── Generate Property Hooks (Code Action, PHP 8.4+) ────────────────────────
// Place the cursor on a property declaration below and trigger "Code Action"
// to see "Generate get hook", "Generate set hook", and "Generate get and set
// hooks".  The property declaration is rewritten to include hook blocks
// inline.  Readonly properties are skipped (PHP 8.4 forbids hooks on readonly
// properties).  Static properties are also skipped.  Interface
// properties generate abstract hook signatures without bodies.  Properties
// that already have one hook only offer the missing one.

class GeneratePropertyHooksDemo
{
    // Cursor here → all three hook actions offered
    public string $title;

    // Cursor here → no hook actions (readonly properties cannot have hooks)
    public readonly int $id;

    // Cursor here → no hook actions (static)
    public static int $counter;

    // Cursor here → only "Generate set hook" (get already exists)
    public string $label {
        get => $this->label;
    }

    // Default values are preserved when hooks are added
    public string $status = 'draft';
}


// ── Change Visibility ───────────────────────────────────────────────────────
// Place cursor on any member and trigger code actions (Ctrl+. / Cmd+.).
// PHPantom offers "Make protected", "Make private", etc.

class ChangeVisibilityDemo
{
    public string $title = '';
    protected int $count = 0;
    private bool $active = true;

    public function getTitle(): string { return $this->title; }
    protected function increment(): void { $this->count++; }
    private function toggle(): void { $this->active = !$this->active; }

    public const VERSION = 1;
    protected const LIMIT = 100;
    private const SECRET = 'shh';

    // Promoted constructor parameters also support visibility change:
    public function __construct(
        private string $name,
        protected int $age,
        public string $role = 'user',
    ) {}
}


// ── Update Docblock ─────────────────────────────────────────────────────────
// Place cursor on a method with a stale docblock and trigger code actions.
// PHPantom offers "Update docblock to match signature" when the @param
// tags are out of sync with the actual parameters.

class UpdateDocblockDemo
{
    /**
     * This docblock is out of date: $old was removed, $added is new,
     * and $renamed had its type changed from string to int.
     *
     * @param string $old This param was removed
     * @param string $renamed Wrong type, should be int
     * @return string Wrong return type, should be array
     */
    public function staleDocblock(int $renamed, bool $added): array
    {
        return [];
    }

    /**
     * Redundant @return void is removed when the signature already says void.
     *
     * @param string $name
     * @return void
     */
    public function redundantReturn(string $name): void {}

    /**
     * Refinement types in docblocks are preserved (not overwritten).
     *
     * @param non-empty-string $label A descriptive label
     * @param array<int, string> $tags Tag list
     */
    public function refinementsPreserved(string $label, array $tags): void {}
}


// ── Extract Function / Method (Code Action) ────────────────────────────────
// Select one or more complete statements inside a method body and trigger
// "Code Action" to see "Extract function" or "Extract method".
//
// Variables defined before the selection become parameters.  Variables
// written inside the selection and read afterwards become return values.
// When $this is used, the code is extracted as a private method.

class ExtractFunctionDemo
{
    private int $factor = 3;

    public function demo(): void
    {
        // Select these two lines and extract:
        // → creates a function with $x as return value (read after selection)
        $x = 10;
        $y = $x * 2;

        echo $x + $y;
    }

    public function methodExtraction(): void
    {
        // Select this line and extract:
        // → creates a private method (uses $this)
        $result = $this->factor * 42;

        echo $result;
    }

    public static function staticExtraction(): void
    {
        // Select these lines and extract:
        // → creates a private static method
        $a = 1;
        $b = 2;

        echo $a + $b;
    }
}


// ── Promote Constructor Parameter ───────────────────────────────────────────
// Place cursor on a constructor parameter (e.g. `string $name`) and trigger
// code actions to see "Promote to constructor property".  The action removes
// the property declaration, removes the `$this->name = $name;` assignment,
// and adds the visibility modifier directly on the parameter.  Attributes on
// the property move onto the parameter with it, so promoting `$slug` gives
// `#[Scaffolding\DemoColumn(type: 'string')] private string $slug`.

class PromoteConstructorParamDemo
{
    private string $name;
    protected int $age;
    private readonly string $email;

    #[Scaffolding\DemoColumn(type: 'string')]
    private string $slug;

    public function __construct(string $name, int $age, string $email, string $slug) {
        $this->name = $name;
        $this->age = $age;
        $this->email = $email;
        $this->slug = $slug;
    }
}

// ── Simplify Null Coalescing / Null-Safe ────────────────────────────────────
// Place your cursor on any ternary below and trigger code actions.
// PHPantom offers "Simplify to ??" or "Simplify to ?->" where applicable.

class SimplifyNullDemo
{
    public function demo(?Scaffolding\Pen $pen, ?Scaffolding\User $user): void
    {
        // ── isset → ?? ─────────────────────────────────────────────
        // Code action: "Simplify to ??"  →  $pen ?? Scaffolding\makePen()
        $tool = isset($pen) ? $pen : Scaffolding\makePen();

        // ── !== null → ?? ──────────────────────────────────────────
        // Code action: "Simplify to ??"  →  $pen ?? Scaffolding\makePen()
        $tool2 = $pen !== null ? $pen : Scaffolding\makePen();

        // ── === null (reversed) → ?? ───────────────────────────────
        // Code action: "Simplify to ??"  →  $user ?? Scaffolding\createUser()
        $fallback = $user === null ? Scaffolding\createUser() : $user;

        // ── !== null + method call → ?-> ───────────────────────────
        // Code action: "Simplify to ?->"  →  $pen?->color()
        $color = $pen !== null ? $pen->color() : null;

        // ── !== null + property access → ?-> ───────────────────────
        // Code action: "Simplify to ?->"  →  $user?->email
        $email = $user !== null ? $user->email : null;

        // ── === null + method (reversed) → ?-> ─────────────────────
        // Code action: "Simplify to ?->"  →  $pen?->label()
        $label = $pen === null ? null : $pen->label();

        // ── Compound subject → correct ?-> placement ───────────────
        // Code action: "Simplify to ?->"  →  $user->getProfile()?->getDisplayName()
        $profile = $user->getProfile();
        $name = $profile !== null ? $profile->getDisplayName() : null;
    }
}


// ── Convert to String Interpolation ─────────────────────────────────────────
// Place your cursor on any concatenation below and trigger code actions.
// PHPantom offers "Convert to string interpolation" when the chain mixes
// literal text with simple variable expressions.

class ConvertToInterpolationDemo
{
    /** @param array<string, string> $row */
    public function demo(string $name, Scaffolding\User $user, Scaffolding\Pen $pen, array $row): string
    {
        // Code action: → "Hello {$name}, welcome!"
        $greeting = 'Hello ' . $name . ', welcome!';

        // Property read.  Code action: → "signed by {$user->email}"
        echo 'signed by ' . $user->email;

        // A method call needs the curly form to interpolate at all.
        // Code action: → "ink: {$pen->color()}"
        echo 'ink: ' . $pen->color();

        // Array read.  Code action: → "<{$row['email']}>"
        $contact = '<' . $row['email'] . '>';

        // Not offered: a numeric operand reads no better interpolated.
        $count = 'items: ' . 12;

        // Not offered: `strlen(…)` does not start with `$`, so PHP would not
        // interpolate it inside `{…}`.
        $length = 'length: ' . strlen($name);

        return $greeting . $contact . $count . $length;
    }
}
