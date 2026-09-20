<?php

/**
 * PHP Showcase — Inlay Hints
 *
 * The inline hints rendered next to arguments and inferred types.
 *
 * One of the demo files listed in README.md. Supporting fixtures live in
 * scaffolding/scaffolding.php (namespace Demo\Scaffolding), and the runtime
 * assertions that verify the type claims in the comments below live in
 * scaffolding/assertions.php.
 */

namespace Demo;

use Demo\Scaffolding;

// ── Inlay Hints ─────────────────────────────────────────────────────────────
// Enable inlay hints in your editor to see parameter names, by-reference
// indicators, and closure type hints. PHPantom shows:
//   - Parameter name hints: greet(/*name:*/ 'Alice', /*age:*/ 25)
//   - By-reference indicators: modify(/*&data:*/ $arr)
//   - Closure param types: $users->map(fn(/*Scaffolding\User*/ $u) => $u->name)
//   - Closure return types: fn($u) /*: string*/ => $u->name
// Hints are suppressed when the argument already makes the parameter obvious
// (e.g. $name matches $name, or a property ->name matches $name).

class InlayHintsDemo
{
    public function demo(): void
    {
        // Parameter name hints appear before each argument:
        $user = Scaffolding\createUser('Alice', 'test@example.com');          // name:, email:

        // By-reference parameters show & before the name:
        $arr = [1, 2, 3];
        $this->modify($arr, 'ascending');         // &data:, direction:

        // Hints are suppressed when variable name matches parameter:
        $needle = 'search term';
        $this->search($needle, 10);               // (no hint for $needle), limit:

        // Constructor calls also get hints:
        $recipe = new Scaffolding\Recipe('Cake', [new Scaffolding\Ingredient('flour', 2)]);  // name:, ingredients:

        // Static method calls:
        Scaffolding\User::findByEmail('alice@example.com');    // email:

        // Chained method calls:
        $pen = Scaffolding\Pen::make('blue');                  // color:
        $pen->rename('Sky Blue');                  // name:

        // ── Closure / arrow function hints ─────────────────────────
        // When a closure or arrow function is passed to a callable-typed
        // parameter, PHPantom infers types from the callable signature.
        // Untyped params show the inferred type before $var, and the
        // return type shows after the closing parenthesis.

        // Arrow function: "User " before $u, ": string" after parens.
        $names = $this->mapUsers(fn($u) => $u->getName());

        // Long-form closure gets the same treatment:
        $upper = $this->mapUsers(function ($u) {
            return strtoupper($u->getName());
        });

        // Partial typing: only the untyped $b gets a hint.
        $sum2 = $this->reduce(fn(int $a, $b) => $a + $b);

        // Already-typed parameters and return types get no hint:
        $emails = $this->mapUsers(fn(Scaffolding\User $u): string => $u->email);

        // Standalone functions with callable params work too:
        $doubled = $this->transformItems([1, 2, 3], fn($x) => $x * 2);

        // Method call context — filter shows "Order " before $o, ": bool" after.
        $big = $this->filterOrders(fn($o) => $o->isAdmin);
    }

    /** @param array<int> &$data */
    public function modify(array &$data, string $direction): void {}

    public function search(string $needle, int $limit = 10): mixed { return null; }

    /**
     * @template T
     * @param array<T> $items
     * @param callable(T): T $fn
     * @return array<T>
     */
    public function transformItems(array $items, callable $fn): array { return $fn(); }

    /** @param \Closure(Scaffolding\User): string $fn */
    public function mapUsers(\Closure $fn): array { return []; }

    /** @param callable(int, int): int $fn */
    public function reduce(callable $fn): int { return 0; }

    /** @param callable(Scaffolding\User): bool $fn */
    public function filterOrders(callable $fn): array { return []; }
}


// ── Reference counts on declarations ────────────────────────────────────────
// Beside a declaration PHPantom shows how many places use it, the same way
// it does for a class, method, property, or constant. A function declared
// outside any class is counted too, so a plain helpers file gets them as
// well — click the count to list the usages.

function inlayFormatLabel(string $text): string
{
    return ucfirst($text);
}

// inlayFormatLabel above shows "2 references": the two calls below.
$inlayFirstLabel = inlayFormatLabel('first');
$inlaySecondLabel = inlayFormatLabel('second');

// A function nothing calls shows "0 references", which is the quickest way
// to spot dead code in a procedural file.
function inlayUnusedHelper(): void {}
