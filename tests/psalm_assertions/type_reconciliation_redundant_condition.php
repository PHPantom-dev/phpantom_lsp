<?php
// Source: Psalm TypeReconciliation/RedundantConditionTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: ignoreIssueAndAssign
namespace PsalmTest_type_reconciliation_redundant_condition_1 {
    function foo(): \stdClass {
        return new \stdClass;
    }

    $b = null;

    foreach ([0, 1] as $i) {
        $a = foo();

        if (!empty($a)) {
            $b = $a;
        }
    }

    assertType('null|stdClass', $b);
}

