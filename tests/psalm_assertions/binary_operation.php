<?php
// Source: Psalm BinaryOperationTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: regularAddition
namespace PsalmTest_binary_operation_1 {
    $a = 5 + 4;

    // Psalm widens two literal operands to `int`; PHPantom folds them to
    // the value PHP computes, as PHPStan does.
    assertType('9', $a);
}

// Test: differingNumericTypesAdditionInWeakMode
namespace PsalmTest_binary_operation_2 {
    $a = 5 + 4.1;

    // Folded, as in regularAddition above.
    assertType('9.1', $a);
}

// Test: modulo
namespace PsalmTest_binary_operation_3 {
    $a = 25 % 2;
    $b = 25.4 % 2;
    $c = 25 % 2.5;
    $d = 25.5 % 2.5;
    $e = 25 % 1;

    assertType('int', $a);
    assertType('int', $b);
    assertType('int', $c);
    assertType('int', $d);
    assertType('int', $e);
}

// Test: concatenationWithTwoLiteralInt
namespace PsalmTest_binary_operation_4 {
    $a = 7 . 5;

    assertType('string', $a);
}

// Test: bitwiseoperations
namespace PsalmTest_binary_operation_5 {
    $a = 4 & 5;
    $b = 2 | 3;
    $c = 4 ^ 3;
    $d = 1 << 2;
    $e = 15 >> 2;
    $f = "a" & "b";

    // Psalm widens a bitwise operation on literals to `int`; PHPantom folds
    // it to the value PHP computes, as PHPStan does, so a mask assigned to a
    // variable can still be bit-tested at the call it is passed to.
    assertType('4', $a);
    assertType('3', $b);
    assertType('7', $c);
    assertType('4', $d);
    assertType('3', $e);
    assertType('string', $f);
}

// Test: booleanXor
namespace PsalmTest_binary_operation_6 {
    $a = 4 ^ 1;
    $b = 3 ^ 1;
    $c = (true xor false);
    $d = (false xor false);

    // Folded, as in the bitwise block above.
    assertType('5', $a);
    assertType('2', $b);
    assertType('bool', $c);
    assertType('bool', $d);
}

// Test: floatIncrement
namespace PsalmTest_binary_operation_7 {
    $a = 1.1;
    $a++;
    $b = 1.1;
    $b += 1;

    assertType('float', $a);
    // Folded, as in regularAddition above.
    assertType('2.1', $b);
}

// Test: exponent
namespace PsalmTest_binary_operation_8 {
    $b = 4 ** 5;

    // Folded, as in regularAddition above.
    assertType('1024', $b);
}

// Test: bitwiseNot
namespace PsalmTest_binary_operation_9 {
    $a = ~4;
    $b = ~4.0;
    $c = ~4.4;
    $d = ~"a";

    assertType('int', $a);
    assertType('int', $b);
    assertType('int', $c);
    assertType('string', $d);
}

// Test: stringIncrementSuppressed
namespace PsalmTest_binary_operation_10 {
    $a = "hello";
    /** @psalm-suppress StringIncrement */
    $a++;

    assertType('string', $a);
}

// Test: numericWithInt
namespace PsalmTest_binary_operation_16 {
    /** @return numeric */
    function getNumeric(){
        return 1;
    }
    $a = getNumeric();
    $a++;
    $b = getNumeric() * 2;
    $c = 1 - getNumeric();
    $d = 2;
    $d -= getNumeric();

    assertType('float|int', $a);
    assertType('float|int', $b);
    assertType('float|int', $c);
    assertType('float|int', $d);
}

// Test: NumericStringIncrementLiteral
namespace PsalmTest_binary_operation_17 {
    $a = "123";
    $b = "123";
    $a++;
    ++$b;

    assertType('float|int', $a);
    assertType('float|int', $b);
}

