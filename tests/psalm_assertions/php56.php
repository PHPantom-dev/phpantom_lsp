<?php
// Source: Psalm Php56Test.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: constArray
namespace PsalmTest_php56_1 {
    const ARR = ["a", "b"];
    $a = ARR[0];

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"a"', $a);
}

// Test: classConstFeatures
namespace PsalmTest_php56_2 {
    class C {
        const ONE = 1;
        const TWO = self::ONE * 2;
        const THREE = self::TWO + 1;
        const ONE_THIRD = self::ONE / self::THREE;
        const SENTENCE = "The value of THREE is " . self::THREE;
        const SHIFT = self::ONE >> 2;
        const SHIFT2 = self::ONE << 1;
        const BITAND = 1 & 1;
        const BITOR = 1 | 1;
        const BITXOR = 1 ^ 1;

        /** @var int */
        public $four = self::ONE + self::THREE;

        /**
         * @param  int $a
         * @return int
         */
        public function f($a = self::ONE + self::THREE) {
            return $a;
        }
    }

    $c1 = C::ONE;
    $c2 = C::TWO;
    $c3 = C::THREE;
    $c1_3rd = C::ONE_THIRD;
    $c_sentence = C::SENTENCE;
    $cf = (new C)->f();
    $c4 = (new C)->four;
    $shift = C::SHIFT;
    $shift2 = C::SHIFT2;
    $bitand = C::BITAND;
    $bitor = C::BITOR;
    $bitxor = C::BITXOR;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('1', $c1);
    assertType('2', $c2);
    assertType('3', $c3);
    assertType('float', $c1_3rd);
    assertType('\'The value of THREE is 3\'', $c_sentence);
    assertType('int', $cf);
    assertType('int', $c4);
    assertType('0', $shift);
    assertType('2', $shift2);
    assertType('1', $bitand);
    assertType('1', $bitor);
    assertType('0', $bitxor);
}

// Test: constFeatures
namespace PsalmTest_php56_3 {
    const ONE = 1;
    const TWO = ONE * 2;
    const BITWISE = ONE & 2;
    const SHIFT = ONE << 2;
    const SHIFT2 = PHP_INT_MAX << 1;

    $one = ONE;
    $two = TWO;
    $bitwise = BITWISE;
    $shift = SHIFT;
    $shift2 = SHIFT2;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('1', $one);
    assertType('2', $two);
    assertType('0', $bitwise);
    assertType('4', $shift);
    assertType('-2', $shift2);
}

