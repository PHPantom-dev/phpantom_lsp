<?php
// Source: Psalm ReturnTypeProvider/BasenameTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: basenameOfStringPathReturnsString
namespace PsalmTest_return_type_provider_basename_1 {
    $base = basename(implode("", range("a", "c")));

    assertType('string', $base);
}

