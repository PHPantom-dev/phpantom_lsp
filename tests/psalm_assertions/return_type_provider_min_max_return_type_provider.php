<?php
// Source: Psalm ReturnTypeProvider/MinMaxReturnTypeProviderTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: literalInt
namespace PsalmTest_return_type_provider_min_max_return_type_provider_1 {
    $min = min(1, 2);
    $max = max(3, 4);

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('1', $min);
    assertType('4', $max);
}

// Test: nonInt
namespace PsalmTest_return_type_provider_min_max_return_type_provider_2 {
    $min = min("a", "b");
    $max = max("x", "y");

    assertType('string', $min);
    assertType('string', $max);
}

