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

// Test: ifNotUndefinedAndEmpty
namespace PsalmTest_type_reconciliation_empty_4 {
    $a = !empty($b) ? $b : null;

    assertType('mixed|null', $a);
}

// Test: issue-9205-1
namespace PsalmTest_type_reconciliation_empty_5 {
    /** @var string $domainCandidate */;

    $candidateLabels = explode('.', $domainCandidate);

    $lastLabel = $candidateLabels[0];

    if (strlen($lastLabel) === 2) {
        exit;
    }

    assertType('string', $lastLabel);
}

// Test: issue-9205-2
namespace PsalmTest_type_reconciliation_empty_6 {
    /** @var string $x */
    if (strlen($x) > 0) {
        exit;
    }

    // PHPantom is more precise than Psalm here: only the empty string survives `strlen($x) > 0` exiting.
    assertType('\'\'', $x);
}

// Test: issue-9205-3
namespace PsalmTest_type_reconciliation_empty_7 {
    /** @var string $x */
    if (strlen($x) === 2) {
        exit;
    }

    assertType('string', $x);
}

// Test: issue-9205-4
namespace PsalmTest_type_reconciliation_empty_8 {
    /** @var string $x */
    if (strlen($x) < 2 ) {
        exit;
    }

    assertType('string', $x);
}

// Test: issue-9349-3
namespace PsalmTest_type_reconciliation_empty_9 {
    /** @var string $a */;
    if (strlen($a) === 7) {
        return $a;
    } elseif (strlen($a) === 10) {
        return $a;
    }

    assertType('string', $a);
}

// Test: issue-9341-1
namespace PsalmTest_type_reconciliation_empty_10 {
    /** @var string */
    $GLOBALS['sql_query'] = rand(0,1) ? 'asd' : null;
    if(!empty($GLOBALS['sql_query']) && mb_strlen($GLOBALS['sql_query']) > 2)
    {
        exit;
    }

    assertType('string', $GLOBALS['sql_query']); // SKIP: an inline `@var` above an array-element assignment is ignored
}
