<?php
// Source: Psalm Php55Test.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: arrayStringDereferencing
namespace PsalmTest_php55_1 {
    $a = [1, 2, 3][0];
    $b = "PHP"[0];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('1', $a);
    assertType("'P'", $b);
}

// Test: classString
namespace PsalmTest_php55_2 {
    class ClassName {}

    $a = ClassName::class;

    // PHPantom types `::class` as the literal name, as PHPStan does, where Psalm widens it to `class-string<ClassName>`.
    assertType("'PsalmTest_php55_2\\\\ClassName'", $a);
}

