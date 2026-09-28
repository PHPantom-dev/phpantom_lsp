<?php
// Source: Psalm TypeReconciliation/IssetTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: isset
namespace PsalmTest_type_reconciliation_isset_1 {
    $a = isset($b) ? $b : null;

    assertType('mixed|null', $a);
}

// Test: nullCoalesce
namespace PsalmTest_type_reconciliation_isset_2 {
    $a = $b ?? null;

    // PHPantom is more precise than Psalm here: `$b` is never assigned, so the coalesce always yields `null`.
    assertType('null', $a);
}

// Test: nullCoalesceWithGoodVariable
namespace PsalmTest_type_reconciliation_isset_3 {
    $b = rand(0, 10) > 5 ? "hello" : null;
    $a = $b ?? null;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"|null', $a);
}

// Test: issetKeyedOffset
namespace PsalmTest_type_reconciliation_isset_4 {
    function getArray() : array {
        return [];
    }

    $foo = getArray();

    if (!isset($foo["a"])) {
        $foo["a"] = "hello";
    }

    assertType('mixed|string', $foo['a']);
}

// Test: nullCoalesceKeyedOffset
namespace PsalmTest_type_reconciliation_isset_5 {
    function getArray() : array {
        return [];
    }

    $foo = getArray();

    $foo["a"] = $foo["a"] ?? "hello";

    assertType('mixed|string', $foo['a']);
}

// Test: issetWithCalculatedKeyAndEqualComparison
namespace PsalmTest_type_reconciliation_isset_6 {
    /** @var array<string, string> $array */
    $array = [];

    function sameString(string $string): string {
        return $string;
    }

    if (isset($array[sameString("key")]) === false) {
        throw new \LogicException("No such key");
    }
    $value = $array[sameString("key")];

    assertType('string', $value);
}

