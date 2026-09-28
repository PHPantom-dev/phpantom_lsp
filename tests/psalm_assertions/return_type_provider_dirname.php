<?php
// Source: Psalm ReturnTypeProvider/DirnameTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: dirnameOfStringPathReturnsString
namespace PsalmTest_return_type_provider_dirname_1 {
    $dir = dirname(implode("", range("a", "c")));

    assertType('string', $dir);
}

// Test: dirnameOfEmptyShouldBeString
namespace PsalmTest_return_type_provider_dirname_2 {
    $foo = rand(0, 1) ? "" : "world";
    $dir = dirname($foo, 20);

    assertType('string', $dir);
}

