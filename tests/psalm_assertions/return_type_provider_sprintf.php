<?php
// Source: Psalm ReturnTypeProvider/SprintfTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: sprintfStringPlaceholderLiteralStringParamFormat
namespace PsalmTest_return_type_provider_sprintf_1 {
    $val = sprintf("%s", "");

    assertType('string', $val);
}

// Test: sprintfStringPlaceholderStringParamFormat
namespace PsalmTest_return_type_provider_sprintf_2 {
    $val = sprintf("%s", implode("", array()));

    assertType('string', $val);
}

// Test: sprintfStringArgnumPlaceholderStringParamsFormat
namespace PsalmTest_return_type_provider_sprintf_3 {
    $val = sprintf("%2\$s%1\$s", "", implode("", array()));

    assertType('string', $val);
}

// Test: sprintfStringPlaceholderIntStringParamFormat
namespace PsalmTest_return_type_provider_sprintf_4 {
    $tmp = rand(0, 10) > 5 ? time() : implode("", array());
    $val = sprintf("%s", $tmp);

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported1
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_7 {
    $val = sprintf('%*.0s', 0, "abc");

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported2
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_8 {
    $val = sprintf('%0.*s', 0, "abc");

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported3
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_9 {
    $val = sprintf('%*.*s', 0, 0, "abc");

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported4
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_10 {
    $precision = 1;
    $flt = 1.234;
    $val = sprintf("%.*f", $precision, $flt);

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported5
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_11 {
    $flt = 1.234;
    $precision = 1;
    $val = sprintf("%1\$.*2\$f", $flt, $precision);

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported6
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_12 {
    $precision = 1;
    $flt = 1.234;
    $val = sprintf("%2\$.*1\$f", $precision, $flt);

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported7
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_13 {
    $flt = 1.234;
    $precision = 1;
    $val = sprintf("%10.*2\$f", $flt, $precision);

    assertType('string', $val);
}

// Test: sprintfComplexPlaceholderNotYetSupported8
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_14 {
    $precision = 1;
    $width = 10;
    $flt = 1.234;
    $val = sprintf("%3\$*2\$.*1\$f", $precision, $width, $flt);

    assertType('string', $val);
}

// Test: sprintfSplatUnpackingArray
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_15 {
    $a = ["a", "b", "c"];
    $val = sprintf("%s%s%s", ...$a);

    assertType('string', $val);
}

// Test: sprintfSplatUnpackingArraySingleArg
// Requires PHP 8.0
namespace PsalmTest_return_type_provider_sprintf_16 {
    $a = ["Hello %s", "Sam"];
    $val = sprintf(...$a);

    assertType('string', $val);
}
