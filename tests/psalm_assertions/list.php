<?php
// Source: Psalm ListTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: simpleVars
namespace PsalmTest_list_1 {
    list($a, $b) = ["a", "b"];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"a"', $a);
    assertType('"b"', $b);
}

// Test: simpleVarsWithSeparateTypes
namespace PsalmTest_list_2 {
    list($a, $b) = ["a", 2];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"a"', $a);
    assertType('2', $b);
}

// Test: simpleVarsWithSeparateTypesInVar
namespace PsalmTest_list_3 {
    $bar = ["a", 2];
    list($a, $b) = $bar;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"a"', $a);
    assertType('2', $b);
}

// Test: mixedNestedAssignment
namespace PsalmTest_list_4 {
    /** @psalm-suppress MissingReturnType */
    function getMixed() {}

    /**
     * @psalm-suppress MixedArrayAccess
     * @psalm-suppress MixedAssignment
     */
    list($a, list($b, $c)) = getMixed();

    assertType('mixed', $a);
    assertType('mixed', $b);
    assertType('mixed', $c);
}

