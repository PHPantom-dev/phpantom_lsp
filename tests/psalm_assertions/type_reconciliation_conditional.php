<?php
// Source: Psalm TypeReconciliation/ConditionalTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: arrayAssignmentPropagation
namespace PsalmTest_type_reconciliation_conditional_1 {
    $dummy = ["test" => 123];

    /** @var array{test: ?int} */
    $a = ["test" => null];

    if ($a["test"] === null) {
        $a = $dummy;
    }
    $var = $a["test"];

    assertType('int', $var);
}

// Test: notInstanceof
namespace PsalmTest_type_reconciliation_conditional_2 {
    class A { }

    class B extends A { }

    $a = new A();

    $out = null;

    if ($a instanceof B) {
        // do something
    }
    else {
        $out = $a;
    }

    assertType('A|null', $out);
}

// Test: notInstanceOfProperty
namespace PsalmTest_type_reconciliation_conditional_3 {
    class B { }

    class C extends B { }

    class A {
        /** @var B */
        public $foo;

        public function __construct() {
            $this->foo = new B();
        }
    }

    $a = new A();

    $out = null;

    if ($a->foo instanceof C) {
        // do something
    }
    else {
        $out = $a->foo;
    }

    assertType('B|null', $out);
}

// Test: notInstanceOfPropertyElseif
namespace PsalmTest_type_reconciliation_conditional_4 {
    class B { }

    class C extends B { }

    class A {
        /** @var string|B */
        public $foo = "";
    }

    $a = new A();

    $out = null;

    if (is_string($a->foo)) {

    }
    elseif ($a->foo instanceof C) {
        // do something
    }
    else {
        $out = $a->foo;
    }

    assertType('B|null', $out);
}

// Test: typeRefinementWithIsNumericOnIntOrString
namespace PsalmTest_type_reconciliation_conditional_5 {
    $a = rand(0, 5) > 4 ? "hello" : 5;

    if (is_numeric($a)) {
      exit;
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"', $a);
}

// Test: typeRefinementWithStringOrTrue
namespace PsalmTest_type_reconciliation_conditional_6 {
    $a = rand(0, 5) > 4 ? "hello" : true;

    if (is_bool($a)) {
      exit;
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"', $a);
}

// Test: ignoreNullCheckAndMaintainNullValue
namespace PsalmTest_type_reconciliation_conditional_7 {
    $a = null;
    if ($a !== null) { }
    $b = $a;

    assertType('null', $b);
}

// Test: ignoreNullCheckAndMaintainNullableValue
namespace PsalmTest_type_reconciliation_conditional_8 {
    $a = rand(0, 1) ? 5 : null;
    if ($a !== null) { }
    $b = $a;

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('null|5', $b);
}

// Test: nullableIntReplacement
namespace PsalmTest_type_reconciliation_conditional_9 {
    $a = rand(0, 1) ? 5 : null;

    $b = (bool)rand(0, 1);

    if ($b || $a !== null) {
        $a = 3;
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('null|3', $a);
}

// Test: isArrayOnArrayKeyOffset
namespace PsalmTest_type_reconciliation_conditional_10 {
    /** @var array{s:array<mixed, array<int, string>|string>} */
    $doc = [];

    if (!is_array($doc["s"]["t"])) {
        $doc["s"]["t"] = [$doc["s"]["t"]];
    }

    assertType('array<int, string>', $doc['s']['t']);
}

// Test: removeStringWithIsScalar
namespace PsalmTest_type_reconciliation_conditional_11 {
    $a = rand(0, 1) ? "hello" : null;

    if (is_scalar($a)) {
        exit;
    }

    assertType('null', $a);
}

// Test: removeNullWithIsScalar
namespace PsalmTest_type_reconciliation_conditional_12 {
    $a = rand(0, 1) ? "hello" : null;

    if (!is_scalar($a)) {
        exit;
    }

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"hello"', $a);
}

// Test: scalarToBool
namespace PsalmTest_type_reconciliation_conditional_13 {
    /** @var scalar */
    $s = 1;

    if (is_bool($s)) {}
    if (!is_bool($s)) {}

    assertType('scalar', $s);
}

// Test: scalarToString
namespace PsalmTest_type_reconciliation_conditional_14 {
    /** @var scalar */
    $s = 1;

    if (is_string($s)) {}
    if (!is_string($s)) {}

    assertType('scalar', $s);
}

// Test: scalarToInt
namespace PsalmTest_type_reconciliation_conditional_15 {
    /** @var scalar */
    $s = 1;

    if (is_int($s)) {}
    if (!is_int($s)) {}

    assertType('scalar', $s);
}

// Test: scalarToFloat
namespace PsalmTest_type_reconciliation_conditional_16 {
    /** @var scalar */
    $s = 1;

    if (is_float($s)) {}
    if (!is_float($s)) {}

    assertType('scalar', $s);
}

// Test: classResolvesBackToSelfAfterComparison
namespace PsalmTest_type_reconciliation_conditional_17 {
    class A {}
    class B extends A {}
    function getA() : A {
      return new A();
    }

    $a = getA();
    if ($a instanceof B) {
        $a = new B;
    }

    assertType('A', $a);
}

// Test: strictIntFloatComparison
namespace PsalmTest_type_reconciliation_conditional_18 {
    /**
     * @psalm-suppress InvalidReturnType
     * @psalm-suppress MismatchingDocblockReturnType
     * @return ($bar is int ? list<int> : list<float>)
     */
    function foo($bar): string {}

    /** @var int */
    $baz = 1;
    $a = foo($baz);

    /** @var float */
    $baz = 1.;
    $b = foo($baz);

    /** @var int|float */
    $baz = 1;
    $c = foo($baz);

    assertType('list<int>', $a);
    assertType('list<float>', $b);
    // PHPantom resolves the conditional return for each member of the `int|float` argument, as PHPStan does, where Psalm merges the two lists.
    assertType('list<int>|list<float>', $c);
}

// Test: ternaryRedefineAllVars
namespace PsalmTest_type_reconciliation_conditional_19 {
    $_a = null;
    $b = rand(0,1) ? "" : "a";
    $b === "a" ? $_a = "Y" : $_a = "N";

    assertType('\'N\'|\'Y\'', $_a);
}

// Test: assertionsWorksBothWays
namespace PsalmTest_type_reconciliation_conditional_20 {
    $a = 2;
    $b = getPositiveInt();

    assert($a === $b);

    /** @return positive-int */
    function getPositiveInt(): int{
        return 2;
    }

    assertType('2', $a);
    assertType('2', $b);
}

// Test: hypotheticalElseDoesNotLeak
namespace PsalmTest_type_reconciliation_conditional_21 {
    $a = 1;
    /** @psalm-suppress RedundantCondition */
    if ($a !== null) {}

    assertType('1', $a);
}

// Test: ifDoesNotLeak
namespace PsalmTest_type_reconciliation_conditional_22 {
    $a = 1;
    /** @psalm-suppress TypeDoesNotContainNull */
    if ($a === null) {}

    assertType('1', $a);
}

// Test: ifElseDoesNotLeak
namespace PsalmTest_type_reconciliation_conditional_23 {
    $a = 1;
    /** @psalm-suppress TypeDoesNotContainNull */
    if ($a === null) {
    } else {
    }

    assertType('1', $a);
}

// Test: ifElseInvertedDoesNotLeak
namespace PsalmTest_type_reconciliation_conditional_24 {
    $a = 1;
    /** @psalm-suppress RedundantCondition */
    if ($a !== null) {
    } else {
    }

    assertType('1', $a);
}

// Test: short_circuited_conditional_test
namespace PsalmTest_type_reconciliation_conditional_25 {
    /** @var ?stdClass $existing */
    $existing = null;

    /** @var bool $foo */
    $foo = true;

    if ($foo) {
    } elseif ($existing === null) {
        throw new \RuntimeException();
    }

    assertType('null|stdClass', $existing);
}

