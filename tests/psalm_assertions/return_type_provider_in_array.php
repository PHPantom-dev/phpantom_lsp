<?php
// Source: Psalm ReturnTypeProvider/InArrayTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: inArrayNonStrictCallReturnsBoolWhenTypesAreCompatible
namespace PsalmTest_return_type_provider_in_array_1 {
    /**
     * @return string[]
     */
    function f(): array {
        return ["1"];
    }
    $ret = in_array("1", f());

    assertType('bool', $ret);
}

// Test: inArrayNonStrictCallReturnsBoolWhenTypesAreIncompatible
namespace PsalmTest_return_type_provider_in_array_2 {
    /**
     * @return string[]
     */
    function f(): array {
        return ["1"];
    }
    $ret = in_array(1, f());

    assertType('bool', $ret);
}

// Test: inArrayStrictCallReturnsBoolWhenTypesAreCompatible
namespace PsalmTest_return_type_provider_in_array_3 {
    /**
     * @return string[]
     */
    function f(): array {
        return ["1"];
    }
    $ret = in_array("1", f(), true);

    assertType('bool', $ret);
}

