<?php
// Source: Psalm ReferenceConstraintTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: paramOutChangeType
namespace PsalmTest_reference_constraint_1 {
    /**
     * @param-out int $s
     */
    function addFoo(?string &$s) : void {
        if ($s === null) {
            $s = 5;
            return;
        }
        $s = 4;
    }

    addFoo($a);

    assertType('4|5', $a);
}

// Test: paramOutReturn
namespace PsalmTest_reference_constraint_2 {
    /**
     * @param-out bool $s
     */
    function foo(?bool &$s) : void {
        $s = true;
    }

    $b = false;
    foo($b);

    // PHPantom narrows the `@param-out` type by what the callee's body writes.
    assertType('true', $b);
}

// Test: PHP80-paramOutChangeTypeWithNamedArgument
namespace PsalmTest_reference_constraint_3 {
    /**
     * @param-out int $s
     */
    function addFoo(bool $five = true, ?string &$s = null) : void {
        if ($five) {
            $s = 5;
            return;
        }
        $s = 4;
    }

    addFoo(s: $a);

    assertType('4|5', $a);
}

