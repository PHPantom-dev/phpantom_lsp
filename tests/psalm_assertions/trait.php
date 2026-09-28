<?php
// Source: Psalm TraitTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: traitSelf
namespace PsalmTest_trait_1 {
    trait T {
        public function g(): self
        {
            return $this;
        }
    }

    class A {
        use T;
    }

    $a = (new A)->g();

    assertType('A', $a);
}

// Test: parentTraitSelf
namespace PsalmTest_trait_2 {
    trait T {
        public function g(): self
        {
            return $this;
        }
    }

    class A {
        use T;
    }

    class B extends A {
    }

    class C {
        use T;
    }

    $a = (new B)->g();

    assertType('A', $a);
}
