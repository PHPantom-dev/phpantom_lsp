<?php
// Source: Psalm ClassLikeStringTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: classStringOfUnionTypeParameter
namespace PsalmTest_class_like_string_1 {
    class A {}
    class B {}

    /**
     * @template T as A|B
     *
     * @param class-string<T> $class
     * @return class-string<T>
     */
    function test(string $class): string {
        return $class;
    }

    $r = test(A::class);

    assertType('class-string<A>', $r);
}
