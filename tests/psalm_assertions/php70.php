<?php
// Source: Psalm Php70Test.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: functionTypeHints
namespace PsalmTest_php70_1 {
    function indexof(string $haystack, string $needle): int
    {
        $pos = strpos($haystack, $needle);

        if ($pos === false) {
            return -1;
        }

        return $pos;
    }

    $a = indexof("arr", "a");

    assertType('int', $a);
}

// Test: methodTypeHints
namespace PsalmTest_php70_2 {
    class Foo {
        public static function indexof(string $haystack, string $needle): int
        {
            $pos = strpos($haystack, $needle);

            if ($pos === false) {
                return -1;
            }

            return $pos;
        }
    }

    $a = Foo::indexof("arr", "a");

    assertType('int', $a);
}

// Test: nullCoalesce
namespace PsalmTest_php70_3 {
    /** @var int $i */
    $i = 0;
    $arr = ["hello", "goodbye"];
    $a = $arr[$i] ?? null;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"|"goodbye"|null', $a);
}

// Test: nullCoalesceWithNullableOnLeft
namespace PsalmTest_php70_4 {
    /** @return ?string */
    function foo() {
        return rand(0, 10) > 5 ? "hello" : null;
    }
    $a = foo() ?? "goodbye";

    assertType('string', $a);
}

// Test: nullCoalesceWithReference
namespace PsalmTest_php70_5 {
    $var = 0;
    ($a =& $var) ?? "hello";

    assertType('0', $a);
}

// Test: spaceship
namespace PsalmTest_php70_6 {
    $a = 1 <=> 1;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('0', $a);
}

// Test: defineArray
namespace PsalmTest_php70_7 {
    define("ANIMALS", [
        "dog",
        "cat",
        "bird"
    ]);

    $a = ANIMALS[1];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"cat"', $a);
}

