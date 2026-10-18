<?php
// Source: Psalm IfThisIsTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: ifThisIsChangeThisTypeInsideMethod
namespace PsalmTest_if_this_is_1 {
    /**
     * @template T
     */
    final class Option
    {
        /**
         * @return T|null
         */
        public function unwrap()
        {
            throw new \RuntimeException("???");
        }
    }

    /**
     * @template T
     */
    final class ArrayList
    {
        /** @var list<T> */
        private $items;

        /**
         * @param list<T> $items
         */
        public function __construct(array $items)
        {
            $this->items = $items;
        }

        /**
         * @psalm-if-this-is ArrayList<Option<int>>
         * @return ArrayList<int>
         */
        public function compact(): ArrayList
        {
            $values = [];

            foreach ($this->items as $item) {
                $value = $item->unwrap();

                if (null !== $value) {
                    $values[] = $value;
                }
            }

            return new self($values);
        }
    }

    /** @var ArrayList<Option<int>> $list */
    $list = new ArrayList([]);
    $numbers = $list->compact();

    assertType('ArrayList<int>', $numbers);
}

// Test: ifThisIsResolveTemplateParams
namespace PsalmTest_if_this_is_2 {
    /**
     * @template-covariant T
     */
    final class Option
    {
        /** @return T|null */
        public function unwrap() { throw new \RuntimeException("???"); }
    }

    /**
     * @template-covariant L
     * @template-covariant R
     */
    final class Either
    {
        /** @return R|null */
        public function unwrap() { throw new \RuntimeException("???"); }
    }

    /**
     * @template T
     */
    final class ArrayList
    {
        /** @var list<T> */
        private $items;

        /**
         * @param list<T> $items
         */
        public function __construct(array $items)
        {
            $this->items = $items;
        }

        /**
         * @template A
         * @template B
         * @template TOption of Option<A>
         * @template TEither of Either<mixed, B>
         *
         * @psalm-if-this-is ArrayList<TOption|TEither>
         * @return ArrayList<A|B>
         */
        public function compact(): ArrayList
        {
            $values = [];

            foreach ($this->items as $item) {
                $value = $item->unwrap();

                if (null !== $value) {
                    $values[] = $value;
                }
            }

            return new self($values);
        }
    }

    /** @var ArrayList<Either<\Exception, int>|Option<int>> $list */
    $list = new ArrayList([]);
    $numbers = $list->compact();

    assertType('ArrayList<int>', $numbers);
}

