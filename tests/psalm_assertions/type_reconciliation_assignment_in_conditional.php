<?php
// Source: Psalm TypeReconciliation/AssignmentInConditionalTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: assertVarRedefinedInOpWithOr
namespace PsalmTest_type_reconciliation_assignment_in_conditional_1 {
    class O {
        public function foo() : bool { return true; }
    }

    /** @var mixed */
    $value = $_GET["foo"];

    $a = !is_string($value) || (($value = rand(0, 1) ? new O : null) === null) || $value->foo();

    assertType('bool', $a);
}

