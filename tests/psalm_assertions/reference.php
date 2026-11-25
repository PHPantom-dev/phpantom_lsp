<?php
// Source: Psalm ReferenceTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: referenceAssignmentToNonReferenceCountsAsUse
namespace PsalmTest_reference_1 {
    $b = &$a;
    $b = 2;
    echo $a;

    assertType('2', $b);
    assertType('2', $a);
}

// Test: updateReferencedTypes
namespace PsalmTest_reference_2 {
    $a = 1;
    $b = &$a;
    $b = 2;
    $c = 3;
    $b = &$c;
    $b = 4;

    assertType('2', $a);
    assertType('4', $b);
    assertType('4', $c);
}

// Test: changingReferencedVariableChangesReference
namespace PsalmTest_reference_3 {
    $a = 1;
    $b = &$a;
    $a = 2;

    assertType('2', $a);
    assertType('2', $b);
}

// Test: unsetReference
namespace PsalmTest_reference_4 {
    $a = 1;
    $b = &$a;
    $b = 2;
    unset($b);
    $b = 3;

    assertType('2', $a);
    assertType('3', $b);
}

// Test: recursiveReference
namespace PsalmTest_reference_5 {
    $a = 1;
    $b = &$a;
    $a = &$b;
    $b = 2;

    assertType('2', $a);
    assertType('2', $b);
}

// Test: referenceToReference
namespace PsalmTest_reference_6 {
    $a = 1;
    $b = &$a;
    $c = &$b;
    $c = 2;
    $d = 3;
    $b = &$d;

    assertType('2', $a);
    assertType('3', $b);
    assertType('2', $c);
    assertType('3', $d);
}

// Test: referenceToNewVariableInitializesNull
namespace PsalmTest_reference_7 {
    $b = &$a;

    assertType('null', $a);
    assertType('null', $b);
}

// Test: referenceToProperty
namespace PsalmTest_reference_8 {
    class Foo
    {
        public string $bar = "";
    }

    $foo = new Foo();
    $bar = &$foo->bar;

    $foo->bar = "bar";

    assertType('\'bar\'', $bar); // SKIP: a write to a property is not seen through a reference to it
}

// Test: allowDocblockTypingOtherVariable
namespace PsalmTest_reference_9 {
    $a = 1;
    /** @var string $a */
    $b = &$a;

    assertType('string', $b);
}

// Test: referenceToArrayVariableOffsetDoesntCrashWhenOffsetVariableChangesDueToReconciliation
namespace PsalmTest_reference_10 {
    $a = "a";
    $b = false;
    $doesNotMatter = ["a" => ["id" => 1]];
    $reference = &$doesNotMatter[$a];
    /** @psalm-suppress TypeDoesNotContainType */
    $result = ($a === "not-a" && ($b || false));

    assertType('array{id: 1}', $reference);
}

// Test: multipleReferencesToArrayVariableOffsetThatChangesDueToReconciliation
namespace PsalmTest_reference_11 {
    $a = "a";
    $b = false;
    $doesNotMatter = ["a" => ["id" => 1]];
    $reference1 = &$doesNotMatter[$a];
    $reference2 = &$doesNotMatter[$a];
    /** @psalm-suppress TypeDoesNotContainType */
    $result = ($a === "not-a" && ($b || false));
    $reference1["id"] = 2;

    assertType('array{id: 2}', $reference1);
    assertType('array{id: 2}', $reference2);
}

