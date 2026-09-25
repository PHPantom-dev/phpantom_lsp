<?php
// Source: Psalm ReturnTypeTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: extendsStaticCallReturnType
namespace PsalmTest_return_type_1 {
    /**
     * @psalm-consistent-constructor
     */
    abstract class BaseLoad {
        /** @return static */
        public static function load() {
            return new static();
        }
    }

    class LoadChild extends BaseLoad {
    }

    $b = LoadChild::load();

    assertType('PsalmTest_return_type_1\LoadChild', $b);
}

// Test: extendsStaticCallArrayReturnType
namespace PsalmTest_return_type_2 {
    /**
     * @psalm-consistent-constructor
     */
    abstract class BaseMulti {
        /** @return array<int,static> */
        public static function loadMultiple() {
            return [new static()];
        }
    }

    class MultiChild extends BaseMulti {
    }

    $bees = MultiChild::loadMultiple();

    assertType('array<int, PsalmTest_return_type_2\MultiChild>', $bees);
}

// Test: overrideReturnType
namespace PsalmTest_return_type_3 {
    class ParentBlah {
        /** @return string|null */
        public function blah() {
            return rand(0, 10) === 4 ? "blah" : null;
        }
    }

    class ChildBlah extends ParentBlah {
        /** @return string */
        public function blah() {
            return "blah";
        }
    }

    $blah = (new ChildBlah())->blah();

    assertType('string', $blah);
}

// Test: interfaceReturnType
namespace PsalmTest_return_type_4 {
    interface Blahable {
        /** @return string|null */
        public function blah();
    }

    class BlahImpl implements Blahable {
        /** @return string|null */
        public function blah() {
            return rand(0, 10) === 4 ? "blah" : null;
        }
    }

    $blah = (new BlahImpl())->blah();

    assertType('null|string', $blah);
}

// Test: overrideReturnTypeInGrandparent
namespace PsalmTest_return_type_5 {
    abstract class A {
        /** @return string|null */
        abstract public function blah();
    }

    abstract class B extends A {
    }

    class C extends B {
        /** @return string|null */
        public function blah() {
            return rand(0, 10) === 4 ? "blahblah" : null;
        }
    }

    $blah = (new C())->blah();

    assertType('null|string', $blah);
}

// Test: infersArrowClosureReturnTypes
// Requires PHP 7.4
namespace PsalmTest_return_type_6 {
    /**
     * @param Closure(int, int): bool $op
     * @return Closure(int): bool
     */
    function reflexive(Closure $op): Closure {
        return fn ($x) => $op($x, $x);
    }

    $res = reflexive(fn(int $a, int $b): bool => $a === $b);

    assertType('Closure(int):bool', $res);
}

// Test: infersObjectShapeOfCastScalar
namespace PsalmTest_return_type_7 {
    function returnsInt(): int {
        return 1;
    }

    $obj = (object)returnsInt();

    // PHPantom is more precise than Psalm here: an object cast always
    // instantiates stdClass, so the result keeps that class identity
    // alongside the shape.
    assertType('object{scalar: int}&stdClass', $obj);
}

// Test: infersObjectShapeOfCastArray
namespace PsalmTest_return_type_8 {
    /**
     * @return array{a:1}
     */
    function returnsArray(): array {
        return ["a" => 1];
    }

    $obj = (object)returnsArray();

    // PHPantom is more precise than Psalm here: an object cast always
    // instantiates stdClass, so the result keeps that class identity
    // alongside the shape.
    assertType('object{a: int}&stdClass', $obj);
}

