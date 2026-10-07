<?php
// Source: Psalm ArgTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: arrayMergeArgumentUnpacking
namespace PsalmTest_arg_1 {
    $a = [[1, 2]];
    $b = array_merge([], ...$a);

    assertType('list{1, 2}', $b);
}

// Test: unpackByRefArg
namespace PsalmTest_arg_2 {
    function example (int &...$x): void {}
    $y = 0;
    example($y);
    $z = [0];
    example(...$z);

    assertType('int', $y);
    assertType('array<int, int>', $z);
}

// Test: sortFunctions
namespace PsalmTest_arg_3 {
    $a = ["b" => 5, "a" => 8];
    ksort($a);
    $b = ["b" => 5, "a" => 8];
    sort($b);
    $c = [];
    sort($c);

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('array{b: 5, a: 8}', $a);
    // Psalm's `list<never>` is the same empty array.
    assertType('array{}', $c);
}

// Test: mixedNullable
// Requires PHP 8.0
namespace PsalmTest_arg_4 {
    class A {
        public function __construct(public mixed $default = null) {
        }
    }
    $a = new A;
    $_v = $a->default;

    assertType('mixed', $_v);
}

