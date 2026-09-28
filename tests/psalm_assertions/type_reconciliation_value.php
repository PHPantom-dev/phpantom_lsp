<?php
// Source: Psalm TypeReconciliation/ValueTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: convertNullArrayKeyToEmptyString
namespace PsalmTest_type_reconciliation_value_1 {
    $a = [
        1 => 1,
        2 => 2,
        null => "hello",
    ];

    $b = $a[""];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"', $b);
}

// Test: literalInt
namespace PsalmTest_type_reconciliation_value_2 {
    $a = (int)"5";

    assertType('5', $a);
}

