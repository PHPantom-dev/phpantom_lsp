<?php
// Source: Psalm TypeReconciliation/ScopeTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: negateAssertionAndOther
namespace PsalmTest_type_reconciliation_scope_1 {
    $a = rand(0, 10) ? "hello" : null;

    if (rand(0, 10) > 1 && is_string($a)) {
        throw new \Exception("bad");
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"|null', $a);
}

// Test: repeatAssertionWithOther
namespace PsalmTest_type_reconciliation_scope_2 {
    function getString() : string {
        return "hello";
    }
    $a = rand(0, 10) ? getString() : null;

    if (rand(0, 10) > 1 || is_string($a)) {
        if (is_string($a)) {
            echo strpos($a, "e");
        }
    }

    assertType('null|string', $a);
}

