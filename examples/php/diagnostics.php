<?php

/**
 * PHP Showcase — Diagnostics
 *
 * The diagnostics PHPantom reports: unknown classes and members, member
 * access on a scalar, argument counts, writes to readonly properties,
 * the properties clone() writes, typed properties nothing initializes,
 * docblocks that contradict their type hint, an invalid class-like kind,
 * and deprecation. These files deliberately contain a fixed set of errors,
 * so the squiggles you see here are the demo, not a mistake.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

use Demo\Scaffolding;

// ── Deprecation Messages ────────────────────────────────────────────────────
// Hover over deprecated members to see the message text from @deprecated.
// When @see tags are present alongside @deprecated, the diagnostic message
// includes the @see references so you know what to migrate to.
// Completion shows deprecated items with strikethrough styling.

class DeprecationDemo
{
    public function demo(): void
    {
        $src = new Scaffolding\ScaffoldingDeprecation();

        // Diagnostic: "'sendLegacy' is deprecated: Use sendAsync() instead.
        //   (see: Scaffolding\ScaffoldingDeprecation::sendAsync())"
        $src->sendLegacy();

        // Diagnostic: "'oldProcess' is deprecated: See: Scaffolding\ScaffoldingDeprecation::sendAsync()"
        // (bare @deprecated + @see → "See:" becomes the main text)
        $src->oldProcess();

        // Diagnostic includes @see reference for the property too
        $src->debugMode;

        // Diagnostic includes @see reference for the constant
        Scaffolding\ScaffoldingDeprecation::OLD_LIMIT;

        // Hover on any constant: shows its value inline (e.g. const MAX_LIMIT = 500;)
        Scaffolding\ScaffoldingDeprecation::MAX_LIMIT;

        // ── #[Deprecated] attribute ─────────────────────────────────
        // PHPantom reads #[Deprecated] from both phpstorm-stubs
        // (\JetBrains\PhpStorm\Deprecated with reason:/since:) and
        // native PHP 8.4 (\Deprecated with message:/since:).

        // JetBrains stubs style: reason: + since:
        $src->attrDeprecatedMethod();

        // Native PHP 8.4 style: message: + since:
        $src->nativeDeprecatedMethod();

        // Bare #[Deprecated] (no arguments)
        $src->attrBareMethod();

        // Positional reason: #[Deprecated("...")]
        $src->attrPositionalMethod();

        // Attribute on property
        $src->attrProp;

        // Attribute on constant
        Scaffolding\ScaffoldingDeprecation::ATTR_OLD;

        // Docblock @deprecated wins when both are present
        $src->bothDocAndAttr();

        // ── Version-aware suppression ───────────────────────────────
        // When #[Deprecated(since: "X.Y")] declares a version and your
        // project targets an older PHP version (via composer.json or
        // .phpantom.toml), the deprecation diagnostic is suppressed.
        // For example, if you target PHP 8.0:
        //   - attrDeprecatedMethod() (since: "8.1") → suppressed
        //   - nativeDeprecatedMethod() (since: "8.4") → suppressed
        //   - sendLegacy() (@deprecated docblock, no since) → still shown

        // ── Replacement code action ─────────────────────────────────
        // When #[Deprecated(replacement: "...")] provides a template,
        // placing the cursor on the call and pressing the quick-fix
        // shortcut offers "Replace with `newFunc(...)`".
        // Template variables: %parametersList%, %parameter0%, %class%.
        $src->legacySetTimezone('UTC');
    }
}


// ── Diagnostic: Unknown Class ───────────────────────────────────────────────
// `MutateArrayInsertSpec` and `Cluster` below are not imported and cannot be
// resolved — they get a yellow "Class 'X' not found" warning underline.
// This diagnostic fires for any ClassReference that PHPantom cannot resolve
// through use-map, local classes, same-namespace, class_index, classmap,
// PSR-4, or stubs.  It pairs with the "Import Class" code action: press
// Ctrl+. (Cmd+. on Mac) on the warning to import the class in one step.

// ── Diagnostic: Unknown Member Access ───────────────────────────────────────
// When PHPantom resolves the subject type but the member does not exist after
// full resolution (inheritance, traits, virtual members), a yellow "Method
// 'X' not found on class 'Y'" warning appears.  Suppressed when __call,
// __callStatic, or __get magic methods are present on the resolved class.

class UnknownMemberDemo
{
    public function demo(): void
    {
        $user = new Scaffolding\User('test', 'test@example.com');

        // These resolve fine — no warning:
        $user->getEmail();
        $user->getName();

        // Try: uncomment the next line to see the warning:
        $user->nonexistentMethod();

        // Static access — unknown constant gets a warning:
        Scaffolding\User::MISSING_CONST;
    }
}


// ── Diagnostic: Destructuring Offsets ───────────────────────────────────────
// `[$a, $b] = …` reads offsets 0 and 1, and `['id' => $id] = …` reads 'id'.
// When the array's type rules the offset out, the target ends up null and PHP
// warns about an undefined key, so PHPantom flags the element.

class DestructuringOffsetDemo
{
    /**
     * @param array<string, int> $totals
     * @param array{id: int, name: string} $row
     */
    public function demo(array $totals, array $row): void
    {
        // No warning: the shape has both keys.
        ['id' => $id, 'name' => $name] = $row;

        // Warning: a string-keyed array has no offsets 0 and 1.
        [$first, $second] = $totals;

        // Warning: the shape has no 'email' key.
        ['email' => $email] = $row;
    }
}


// ── Diagnostic: Scalar Member Access ────────────────────────────────────────
// Accessing a property or calling a method on a scalar type (int, string,
// bool, float, null, void, never) is always a runtime error.  PHPantom flags
// these with an Error-severity diagnostic, including through method-return
// chains.

class ScalarMemberAccessDemo
{
    public function demo(Scaffolding\User $user): void
    {
        // getName() returns string — accessing a method on it is an error:
        $user->getName()->trim();

        // getEmail() returns string — property access is also an error:
        $user->getEmail()->length;

        // Chains through intermediate classes work too:
        $user->getProfile()->getDisplayName()->toUpper();

        // Works with Scaffolding\Response too — isSuccess() returns bool:
        $resp = new Scaffolding\Response(200, 'OK');
        $resp->isSuccess()->flag;

        // A `::` access is different: `$string::method()` is valid PHP
        // (the string names a class at runtime), so it is left
        // unresolved rather than flagged — see ClassStringPropertyDemo in
        // completion.php
        // below. A scalar that can never name a class, like `int`,
        // still triggers this same diagnostic through `::`:
        $user->age::method();
    }
}


// ── Diagnostic: Unresolved Member Access (opt-in) ───────────────────────────
// When PHPantom cannot resolve the *subject type* of a member access at all,
// it can show a hint-level diagnostic.  This is off by default because most
// codebases lack full type coverage.  Enable it in .phpantom.toml:
//
//   [diagnostics]
//   unresolved-member-access = true
//
// This is useful for discovering gaps in type coverage or places where
// PHPantom's inference falls short.

class UnresolvedMemberAccessDemo
{
    public function demo(): void
    {
        // $mystery has type "mixed" — PHPantom cannot resolve it.
        // With the diagnostic enabled, a hint appears on the next line:
        $mystery = Scaffolding\getUnknownValue();
        $mystery->doSomething();
    }
}


// ── Diagnostic: Partial Union Members (opt-in) ──────────────────────────────
// A member that only some classes of a union declare fails whenever one of
// the others arrives.  PHPantom stays silent about it by default, since the
// member exists on at least one branch.  Enable it in .phpantom.toml:
//
//   [diagnostics]
//   partial-union-members = true

class PartialUnionMembersDemo
{
    public function demo(Scaffolding\Pen|Scaffolding\Pencil $tool): void
    {
        $tool->label();     // fine: both Pen and Pencil declare it
        // With the diagnostic enabled, a warning appears on the next line:
        $tool->write();     // Method 'write' exists on Pen but not on Pencil

        if ($tool instanceof Scaffolding\Pen) {
            $tool->write(); // fine: narrowed to Pen
        }
    }
}


// ── Diagnostic: Argument Count ──────────────────────────────────────────────
// PHPantom flags calls that pass too few or too many arguments.  Variadic
// parameters accept unlimited trailing args.  Argument unpacking (`...$args`)
// suppresses the diagnostic because the actual count is unknown statically.

class ArgumentCountDemo
{
    public function demo(): void
    {
        $user = new Scaffolding\User('Alice', 'alice@test.com');

        // Correct — no diagnostic:
        $user->getEmail();
        $user->setName('Bob');
        $user->addRoles('admin', 'editor', 'viewer'); // variadic

        // Too few arguments — error diagnostic appears:
        $user->setStatus();

        // Too many arguments — error diagnostic appears:
        $user->getEmail('extra');
    }
}

class TypeErrorDemo
{
    public function demo(): void
    {
        $user = new Scaffolding\User('Alice', 'alice@test.com');

        // Correct — no diagnostic:
        $user->setName('Bob');
        $user->setStatus(Scaffolding\Status::Active);

        // Type error — string passed to int parameter:
        $this->requiresInt("not a number");

        // Type error — null passed to non-nullable parameter:
        $this->requiresString(null);

        // Type error — wrong class type:
        $pen = new Scaffolding\Pen('blue');
        $this->requiresUser($pen);

        // No diagnostic — subclass is compatible:
        $admin = new Scaffolding\AdminUser('Admin', 'admin@test.com', ['manage']);
        $this->requiresUser($admin);

        // Type error — the box's type argument is inferred from what it
        // was constructed with, so a box of Scaffolding\Pen is not a box of Scaffolding\User:
        $this->requiresUserBox(new Scaffolding\ScaffoldingTypedBox($pen));

        // No diagnostic — a box of a Scaffolding\User subclass satisfies it:
        $this->requiresUserBox(new Scaffolding\ScaffoldingTypedBox($admin));

        // Type error — this file does not declare `strict_types`, so a
        // string handed to an `int` parameter would be coerced, but the
        // `string` inside a box never is: the box is passed on whole:
        $this->requiresIntBox(new Scaffolding\ScaffoldingTypedBox('7'));

        // No diagnostic — the box holds what was asked for:
        $this->requiresIntBox(new Scaffolding\ScaffoldingTypedBox(7));

        // Type error — `interface-string` constrains the name the string
        // holds, so a class that implements the interface is still the
        // wrong kind of name:
        $this->requiresInterfaceName(Scaffolding\SealedEnvelope::class);

        // No diagnostic — the name of the interface itself:
        $this->requiresInterfaceName(Scaffolding\Printable::class);

        // No diagnostic — null is valid for nullable parameter:
        $this->acceptsNullable(null);
        $this->acceptsNullable("hello");

        // No diagnostic — int widens to float:
        $this->requiresFloat(42);

        // Type error — a `void` method hands back no value.  PHP 8 passes
        // the `null` it substitutes, so the call site's misreading of the
        // API surfaces inside the callee rather than here:
        $this->requiresString($this->logAction('saved'));

        // Type error — a `callable(...)` parameter states what the callee
        // will do with the result, so a closure that returns something
        // else breaks the contract on the first call:
        $this->requiresUserMatcher(static fn (Scaffolding\User $u): string => $u->getName());

        // No diagnostic — the closure returns what the parameter promised:
        $this->requiresUserMatcher(static fn (Scaffolding\User $u): bool => $u->getName() !== '');

        // Type error — the closure declares no return type, but its body
        // resolves to one, and that is just as much a contradiction:
        $this->requiresUserMatcher(static fn (Scaffolding\User $u) => $u->getName());

        // No diagnostic — a bare `Closure` carries no signature at all,
        // so there is nothing to hold it to:
        $this->requiresUserMatcher($this->makeMatcher());

        // Type error — an array written out here lists every key it has,
        // so a required shape key that is not among them is missing:
        $this->requiresConfig(['host' => 'localhost']);

        // No diagnostic — every required key is there. Order does not
        // matter, and `timeout` is optional:
        $this->requiresConfig(['port' => 3306, 'host' => 'localhost']);

        // Type error — a shape without a trailing `...` declares every key
        // it handles, so `debug` is one it does not accept:
        $this->requiresConfig(['host' => 'localhost', 'port' => 3306, 'debug' => true]);

        // No diagnostic — a shape built up over several statements records
        // the keys we watched being assigned, which is a lower bound on
        // what the array holds rather than the whole of it:
        $config = ['host' => 'localhost'];
        if ($this->useDefaultPort()) {
            $config['port'] = 3306;
        }
        $this->requiresConfig($config);

        // Type error — a `list` holds keys `0, 1, 2, …` in that order,
        // which is what `array_is_list()` answers `true` for, and keys
        // written the other way round are kept the way they are written:
        $this->requiresPair([1 => 'right', 0 => 'left']);

        // No diagnostic — both spellings of the same list:
        $this->requiresPair(['left', 'right']);
        $this->requiresPair([0 => 'left', 1 => 'right']);
    }

    private function requiresInt(int $value): void {}
    private function requiresString(string $text): void {}
    private function requiresUser(Scaffolding\User $user): void {}
    /** @param Scaffolding\ScaffoldingTypedBox<Scaffolding\User> $box */
    private function requiresUserBox(Scaffolding\ScaffoldingTypedBox $box): void {}
    /** @param Scaffolding\ScaffoldingTypedBox<int> $box */
    private function requiresIntBox(Scaffolding\ScaffoldingTypedBox $box): void {}
    /** @param interface-string $name */
    private function requiresInterfaceName(string $name): void {}
    private function acceptsNullable(?string $text): void {}
    private function requiresFloat(float $value): void {}
    private function logAction(string $message): void {}
    /** @param callable(Scaffolding\User): bool $matcher */
    private function requiresUserMatcher(callable $matcher): void {}
    /** @param array{host: string, port: int, timeout?: int} $config */
    private function requiresConfig(array $config): void {}
    /** @param list{string, string} $pair */
    private function requiresPair(array $pair): void {}
    private function useDefaultPort(): bool
    {
        return true;
    }
    private function makeMatcher(): \Closure
    {
        return static fn (Scaffolding\User $u): bool => $u->getName() !== '';
    }
}


// ── Diagnostic: Readonly Property Writes ────────────────────────────────────
// A `readonly` property may be initialized once, and only from inside the
// class that declares it.  PHPantom flags every other write.  Writes the
// language allows (the constructor initializing its own properties, a
// property the constructor may leave uninitialized, `__clone` reinitializing
// one on PHP 8.3+) are left alone.

class ReadonlyWriteDemo
{
    public function __construct(public readonly int $version = 1) {}

    public function bump(): void
    {
        // Error — the constructor already initialized `$version`:
        $this->version++;
    }

    public function demo(): void
    {
        $coordinate = new Scaffolding\ScaffoldingCoordinate(1, 2);

        // Error — the write happens outside the declaring class:
        $coordinate->x = 10;

        // Error — compound operators and increments modify it too:
        $coordinate->y += 5;

        // Error — `unset()` counts as a write:
        unset($coordinate->x);

        // Error — so does a destructuring target:
        [$coordinate->x, $coordinate->y] = [1, 2];

        // Error — and so does taking a reference:
        $alias = &$coordinate->x;
        echo $alias;

        // Error — the array a readonly property holds cannot be modified
        // either, however the write is spelled:
        $coordinate->tags[] = 'origin';

        // Error — every property of a `readonly` class is readonly, with or
        // without the keyword on the property itself:
        $point = new Scaffolding\ScaffoldingReadonlyPoint(1, 2);
        $point->x = 10;

        // No diagnostic — reading is always fine:
        echo $coordinate->x + $coordinate->y;

        // No diagnostic — `$label` is writable:
        $coordinate->label = 'origin';
    }
}


// ── Diagnostic: Properties clone() Writes ───────────────────────────────────
// PHP 8.5's `clone($object, [...])` sets each listed property on the copy,
// with the same rules as an assignment from the calling code.  PHPantom
// checks every literal key like `$copy->key = value`.
//
// Try: rename `label` from its key below; the property follows.

class CloneWithDemo
{
    public function demo(): void
    {
        $coordinate = new Scaffolding\ScaffoldingCoordinate(1, 2);

        // No diagnostic — `$label` is a writable string property:
        $copy = clone($coordinate, ['label' => 'copy']);
        echo $copy->label;

        // Error — the class has no `$z` property:
        $copy = clone($coordinate, ['z' => 3]);

        // Error — `$x` is readonly, and this is not its declaring class:
        $copy = clone($coordinate, ['x' => 10]);

        // Error — `$label` expects a string:
        $copy = clone($coordinate, ['label' => []]);
    }
}


// ── Diagnostic: Uninitialized Properties ────────────────────────────────────
// A typed property without a default starts uninitialized, and reading it
// before anything writes it throws.  PHPantom flags a property that only the
// class itself may write (private, protected in a final class, or readonly)
// when nothing in the class ever writes it.  A write from any method counts,
// and so does a framework that fills the property: an attribute, or a
// Doctrine generated identifier.  A public property is left to whoever
// creates the object, so it is flagged where an object fresh from `new` is
// read before anything writes it.

final class UninitializedPropertyDemo
{
    // Warning — nothing ever writes `$createdAt`:
    private \DateTimeImmutable $createdAt;

    // No diagnostic — the constructor initializes `$name`:
    private string $name;

    // No diagnostic — `setNote()` writes `$note`:
    private string $note;

    public readonly int $id;

    // No diagnostic here — whoever creates the object may write `$label`:
    public string $label;

    public function __construct(string $name, int $id)
    {
        $this->name = $name;
        $this->id = $id;

        // Error — the constructor already initialized `$id`:
        $this->id = $id + 1;
    }

    public function setNote(string $note): void
    {
        $this->note = $note;
    }

    public function describe(): string
    {
        return $this->name . ': ' . $this->note . ' ' . $this->createdAt->format('Y');
    }
}

final class UninitializedReadDemo
{
    public function labelled(): string
    {
        $demo = new UninitializedPropertyDemo('demo', 1);

        // Warning — nothing wrote `$label` before this read:
        $label = $demo->label;

        $demo->label = 'Demo';

        // No diagnostic — `$label` is written now:
        return $label . $demo->label;
    }
}


// ── Diagnostic: Discarded #[\NoDiscard] Results ─────────────────────────────
// PHP 8.5 warns when a function or method marked `#[\NoDiscard]` is called as
// a statement of its own, throwing its result away.  Cast the call to
// `(void)` to discard the result on purpose.

class DiscardedResultDemo
{
    #[\NoDiscard]
    public function reserveSlot(): int
    {
        return 42;
    }

    public function demo(): void
    {
        // Warning — the reserved slot number is thrown away:
        $this->reserveSlot();

        // No diagnostic — the result is used:
        echo $this->reserveSlot();

        // No diagnostic — discarded on purpose:
        (void) $this->reserveSlot();
    }
}


// ── Diagnostic: Unhandled Match Values ──────────────────────────────────────
// A `match` without a `default` arm throws `UnhandledMatchError` for any value
// none of its arms names.  When the subject is an enum, a `bool`, or a set of
// literals, the values no arm handles are reported.

class UnhandledMatchDemo
{
    public function badge(Scaffolding\Status $status): string
    {
        // Error — Status::Pending has no arm:
        return match ($status) {
            Scaffolding\Status::Active => 'green',
            Scaffolding\Status::Inactive => 'grey',
        };
    }

    public function label(Scaffolding\Status $status): string
    {
        // No diagnostic — the guard already ruled Status::Pending out:
        if ($status === Scaffolding\Status::Pending) {
            return 'waiting';
        }
        return match ($status) {
            Scaffolding\Status::Active => 'on',
            Scaffolding\Status::Inactive => 'off',
        };
    }
}


// ── Diagnostic: Dynamic Member Names ────────────────────────────────────────
// A member name written as an expression has to be usable as a name.  A
// property name is converted to a string, so a `Stringable` object works and
// any other object does not.  A method name must already be a string: PHP
// never calls `__toString()` for it.

class DynamicMemberNameDemo
{
    public string $title = 'demo';

    public function names(Scaffolding\ScaffoldingFieldLabel $label, \stdClass $bag, string $method): void
    {
        // No diagnostic — the label converts to the property name 'title':
        echo $this->{$label};

        // Error — a stdClass cannot be converted to a string:
        echo $this->{$bag};

        // Error — PHP does not convert a method name, even a Stringable one:
        $this->{$label}();

        // No diagnostic — a string names the method:
        $this->{$method}();
    }
}


// ── Diagnostic: Member Visibility ───────────────────────────────────────────
// A member that exists is not automatically a member you may touch.  PHP
// resolves the access to a declaration first and enforces that declaration's
// visibility second, so reaching a `private` or `protected` member from a
// scope that cannot see it is a fatal error.  The check is made against the
// class that *declares* the member, not the one the access went through.

class MemberVisibilityDemo extends Scaffolding\ScaffoldingVault
{
    public function insideTheHierarchy(): void
    {
        // No diagnostic — a subclass sees protected members:
        echo $this->branch;
        echo $this->audit();
        echo static::ROTATION;

        // Error — PHP does not inherit private members, so `$pin` belongs to
        // the parent alone:
        echo $this->pin;
    }

    public function fromOutside(): void
    {
        $vault = new Scaffolding\ScaffoldingVault();

        // No diagnostic — public members are always reachable:
        echo $vault->label;
        echo $vault->open();
        echo Scaffolding\ScaffoldingVault::REGION;

        // No diagnostic — `$branch` is protected and declared on the class
        // this one descends from, so it is in scope even on another instance:
        echo $vault->branch;

        // Error — the property is private to the class that declares it:
        echo $vault->pin;

        // Error — a method is checked the same way:
        echo $vault->rotate();

        // Error — so is a class constant:
        echo Scaffolding\ScaffoldingVault::MASTER_KEY;

        // Error — and a static property:
        echo Scaffolding\ScaffoldingVault::$openCount;

        // Error — `$ledger` is private to the subclass that declares it, and
        // this class is a sibling of that one rather than a descendant:
        $branch = new Scaffolding\ScaffoldingBranchVault();
        echo $branch->ledger;
    }
}


// ── Diagnostic: Unreachable Code ────────────────────────────────────────────
// A statement after one that always leaves the block never runs.  These are
// dimmed rather than underlined: dead code is tidying, not a defect.  The
// check reads the shape of the statement list only, so it needs no type
// resolution and keeps up with typing.

class UnreachableCodeDemo
{
    public function afterReturn(): void
    {
        return;

        // Dimmed — nothing after the return can run:
        echo 'never';
    }

    public function everyBranchLeaves(bool $flag): string
    {
        if ($flag) {
            return 'yes';
        } elseif ($flag) {
            throw new \RuntimeException('impossible');
        } else {
            return 'no';
        }

        // Dimmed — all three branches leave, so nothing follows them:
        echo 'never';
    }

    public function oneBranchFallsThrough(bool $flag): void
    {
        if ($flag) {
            return;
        }

        // No diagnostic — an `if` with no `else` always has a path that
        // reaches here:
        echo 'reached when the flag is false';
    }

    public function insideALoop(array $items): void
    {
        foreach ($items as $item) {
            if ($item === null) {
                continue;

                // Dimmed — `continue` leaves the iteration:
                echo 'never';
            }
        }

        // No diagnostic — the loop body may never run at all, so what
        // follows the loop is reachable:
        echo 'reached';
    }

    public function nestedDeclaration(): void
    {
        return;

        // Dimmed — a declaration inside a function body is created by
        // running the statement, so this one never comes into being.
        // A `function` or `class` written at the top level of a file is
        // hoisted instead, and is left alone there.
        function neverDeclared(): void {}
    }

    public function jumpTarget(): void
    {
        goto tail;

        // Dimmed — unreachable by falling through:
        echo 'never';

        tail:

        // No diagnostic — a label is an entry point, so the dead run ends
        // here:
        echo 'reached by the jump';
    }
}


// ── Diagnostic: Docblock Contradicts the Type Hint ──────────────────────────
// A `@param` or `@return` tag refines the native declaration; it must not
// disagree with it.  When the signature admits `null` and the tag does not,
// the two describe different sets of values: a caller reading the signature
// may legally pass `null`, and the callee has been promised it never receives
// one.  A tag that keeps the `null` while narrowing the rest is the annotation
// doing its job and is left alone, and so is a name PHPantom cannot settle,
// since a `@template` parameter or an imported type alias may itself be
// nullable.

class DocblockNativeMismatchDemo
{
    private ?string $path = null;

    /**
     * @param string $name
     */
    // Warning — `string` denies the null that `?string` accepts:
    public function greet(?string $name): void
    {
        echo $name;
    }

    /**
     * @param list<int> $items
     */
    // Warning — an array type reads the same way:
    public function takesItems(?array $items): void
    {
        echo count($items ?? []);
    }

    /**
     * @return string
     */
    // Warning — on the return type this time:
    public function getPath(): ?string
    {
        return $this->path;
    }

    /**
     * @param list<int>|null $items
     * @return non-empty-string|null
     */
    // No diagnostic — both tags narrow the type and keep the null:
    public function narrowedButStillNullable(?array $items): ?string
    {
        return $items === null ? null : 'ok';
    }

    /**
     * @param list<int> $items
     */
    // No diagnostic — nothing in the signature admits null to begin with:
    public function requiresItems(array $items): void
    {
        echo count($items);
    }
}


// ── Invalid Class-Like Kind Diagnostics ─────────────────────────────────────
// PHPantom flags class-like names used in positions where their kind is
// guaranteed to fail at runtime.  Open demo() and look for Error/Warning
// squiggles on the class names.

class InvalidClassKindDemo
{
    public function demo(): void
    {
        // Error: cannot instantiate abstract class
        $a = new Scaffolding\ScaffoldingAbstractShape();

        // Error: cannot instantiate enum
        $b = new Scaffolding\Status();

        // Warning: instanceof with a trait always evaluates to false
        $x = new Scaffolding\Pen('test');
        $result = $x instanceof Scaffolding\JsonSerializer;

        // Warning: trait in a type hint will always fail type checking
        $this->acceptTrait(new Scaffolding\Pen('test'));
    }

    private function acceptTrait(Scaffolding\JsonSerializer $x): Scaffolding\JsonSerializer
    {
        return $x;
    }

    // These also produce diagnostics but would crash at class-load time,
    // so they are commented out.  See the AGENTS.md hoisting pitfall note.
}


// ── Diagnostic: #[\Override] With Nothing to Override ───────────────────────
// PHP refuses to load a class whose method (8.3+) or property (8.5+) is
// marked #[\Override] when no parent class or interface declares it, so the
// mistake is reported as an error.

abstract class OverrideAttributeDemo extends Scaffolding\Model
{
    #[\Override]
    public function getName(): string // fine: Model declares getName()
    {
        return 'demo';
    }

    // Try: uncomment the method below to see the error (PHP would refuse
    // to load the class, so it stays commented out):
    // #[\Override]
    // public function getNickname(): string { return 'demo'; }
}
