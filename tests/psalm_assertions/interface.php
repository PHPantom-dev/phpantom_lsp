<?php
// Source: Psalm InterfaceTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: extendsAndImplements
namespace PsalmTest_interface_1 {
    interface A
    {
        /**
         * @return string
         */
        public function fooFoo();
    }

    interface B
    {
        /**
         * @return string
         */
        public function barBar();
    }

    interface C extends A, B
    {
        /**
         * @return string
         */
        public function baz();
    }

    class D implements C
    {
        /**
         * @return string
         */
        public function fooFoo()
        {
            return "hello";
        }

        /**
         * @return string
         */
        public function barBar()
        {
            return "goodbye";
        }

        /**
         * @return string
         */
        public function baz()
        {
            return "hello again";
        }
    }

    $cee = (new D())->baz();
    $dee = (new D())->fooFoo();

    assertType('string', $cee);
    assertType('string', $dee);
}

