<?php
// Source: Psalm Template/KeyOfTemplateTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: mixedKeyOf
namespace PsalmTest_template_key_of_template_1 {
    /** @var array<int>|mixed */
    $a = [];

    /** @psalm-suppress MixedArgument */
    $a = array_keys($a);

    assertType('list<array-key>', $a);
}

