<?php
// Source: Psalm GeneratorTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: generator
namespace PsalmTest_generator_1 {
    /**
     * @param  int  $start
     * @param  int  $limit
     * @param  int  $step
     * @return \Generator<int>
     */
    function xrange($start, $limit, $step = 1) {
        for ($i = $start; $i <= $limit; $i += $step) {
            yield $i;
        }
    }

    $a = null;

    /*
     * Note that an array is never created or returned,
     * which saves memory.
     */
    foreach (xrange(1, 9, 2) as $number) {
        $a = $number;
    }

    assertType('int|null', $a);
}

// Test: generatorReturnType
namespace PsalmTest_generator_2 {
    /** @return \Generator<int, \stdClass> */
    function g():\Generator { yield new \stdClass; }

    $g = g();

    assertType('Generator<int, stdClass, mixed, mixed>', $g);
}

// Test: generatorDelegation
namespace PsalmTest_generator_3 {
    /**
     * @return \Generator<int, int, mixed, int>
     */
    function count_to_ten(): \Generator {
        yield 1;
        yield 2;
        yield from [3, 4];
        yield from new \ArrayIterator([5, 6]);
        yield from seven_eight();
        return yield from nine_ten();
    }

    /**
     * @return \Generator<int, int>
     */
    function seven_eight(): \Generator {
        yield 7;
        yield from eight();
    }

    /**
     * @return \Generator<int,int>
     */
    function eight(): \Generator {
        yield 8;
    }

    /**
     * @return \Generator<int,int, mixed, int>
     */
    function nine_ten(): \Generator {
        yield 9;
        return 10;
    }

    $gen = count_to_ten();
    foreach ($gen as $num) {
        echo "$num ";
    }
    $gen2 = $gen->getReturn();

    assertType('Generator<int, int, mixed, int>', $gen);
    assertType('int', $gen2);
}

// Test: fillTemplatesForIteratorFromGenerator
namespace PsalmTest_generator_4 {
    /**
     * @return \Generator<int, string>
     */
    function generator(): \Generator
    {
        yield "test";
    }

    $iterator = new \NoRewindIterator(generator());

    assertType('NoRewindIterator<int, string, Generator<int, string, mixed, mixed>>', $iterator);
}

