<?php
// Source: Psalm TypeReconciliation/EmptyTest.php
namespace PsalmTest_type_reconciliation_empty_1 {
    /** @param mixed $a */
    function foo($a): void {
        if (empty($a)) {
            assertType("0|0.0|''|'0'|array{}|false|null", $a);
        }
    }
}

namespace PsalmTest_type_reconciliation_empty_2 {
    /** @param string $a */
    function foo($a): void {
        if (!empty($a)) {
            assertType('string', $a);
        }
    }
}

namespace PsalmTest_type_reconciliation_empty_3 {
    /** @param string|null $a */
    function foo($a): void {
        if (!empty($a)) {
            assertType('string', $a);
        }
    }
}