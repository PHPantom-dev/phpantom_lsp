<?php
// Source: Psalm ArrayKeysTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: arrayKeysOfEmptyArrayReturnsListOfEmpty
namespace PsalmTest_array_keys_1 {
    $keys = array_keys([]);

    assertType('list<never>', $keys);
}

// Test: intStringKeyAsInt
namespace PsalmTest_array_keys_2 {
    $a = ["15" => "a"];
    $b = ["15.7" => "a"];
    // since PHP 8 this is_numeric but will not be int key
    $c = ["15 " => "a"];
    $d = ["-15" => "a"];
    // see https://github.com/php/php-src/issues/9029#issuecomment-1186226676
    $e = ["+15" => "a"];
    $f = ["015" => "a"];
    $g = ["1e2" => "a"];
    $h = ["1_0" => "a"];

    assertType('array{15: \'a\'}', $a);
    assertType('array{\'15.7\': \'a\'}', $b);
    assertType('array{\'15 \': \'a\'}', $c);
    assertType('array{-15: \'a\'}', $d);
    assertType('array{\'+15\': \'a\'}', $e);
    assertType('array{\'015\': \'a\'}', $f);
    assertType('array{\'1e2\': \'a\'}', $g);
    assertType('array{\'1_0\': \'a\'}', $h);
}

