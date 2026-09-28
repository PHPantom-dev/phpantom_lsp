<?php
// Source: Psalm MethodSignatureTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: staticReturnShouldBeStatic
namespace PsalmTest_method_signature_1 {
    class A {
        /** @return static */
        public static function foo() {
            return new static();
        }

        final public function __construct() {}
    }

    class B extends A {
        public static function foo() {
            return new static();
        }
    }

    $b = B::foo();

    assertType('B', $b);
}

// Test: allowLessSpecificDocblockTypeOnParent
namespace PsalmTest_method_signature_2 {
    abstract class Foo {
        /**
         * @return array|string
         */
        abstract public function getTargets();
    }

    class Bar extends Foo {
        public function getTargets(): string {
            return "baz";
        }
    }

    $a = (new Bar)->getTargets();

    assertType('string', $a);
}

