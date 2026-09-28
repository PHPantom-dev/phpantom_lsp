<?php
// Source: Psalm MatchTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: nullCoalesce
// Requires PHP 8.0
namespace PsalmTest_match_1 {
    function foo(): bool { return false; }
    $match = match (foo()) {
        false => null,
        true => 1,
    } ?? 2;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('1|2', $match);
}

