<?php
// Source: Psalm DocblockInheritanceTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: inheritParentReturnDocbblock
namespace PsalmTest_docblock_inheritance_1 {
    class Foo {
        /**
         * @return int[]
         */
        public function doFoo() {
            return [1, 2, 3];
        }
    }

    class Bar extends Foo {
        public function doFoo(): array {
            return [4, 5, 6];
        }
    }

    $b = (new Bar)->doFoo();

    assertType('array<array-key, int>', $b);
}

