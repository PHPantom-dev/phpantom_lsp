<?php
// Source: Psalm MixinAnnotationTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: validSimpleAnnotations
namespace PsalmTest_mixin_annotation_1 {
    class ParentClass {
        public function __call(string $name, array $args) {}
        public static function __callStatic(string $name, array $args) {}
    }

    class Provider {
        public function getString() : string {
            return "hello";
        }

        public function setInteger(int $i) : void {}

        public static function getInt() : int {
            return 5;
        }
    }

    /** @mixin Provider */
    class Child extends ParentClass {}

    $child = new Child();

    $a = $child->getString();
    $b = $child::getInt();

    assertType('string', $a);
    assertType('int', $b);
}

// Test: wrapCustomIterator
namespace PsalmTest_mixin_annotation_2 {
    /**
     * @implements \Iterator<1, 2>
     */
    class Subject implements \Iterator {
        /**
         * the index method exists
         *
         * @param int $index
         * @return bool
         */
        public function index($index) {
            return true;
        }

        public function current() {
            return 2;
        }

        public function next() {}

        public function key() {
            return 1;
        }

        public function valid() {
            return false;
        }

        public function rewind() {}
    }

    $iter = new \IteratorIterator(new Subject());
    $b = $iter->index(0);

    assertType('bool', $b);
}

// Test: templatedMixin
namespace PsalmTest_mixin_annotation_3 {
    /**
     * @template T
     */
    abstract class Foo {
        /** @return T */
        abstract public function hi();
    }

    /**
     * @mixin Foo<string>
     */
    class Bar {}

    $bar = new Bar();
    $b = $bar->hi();

    assertType('string', $b);
}

// Test: multipleMixins
namespace PsalmTest_mixin_annotation_4 {
    class MixinA {
        function a(): string { return "foo"; }
    }

    class MixinB {
        function b(): int { return 0; }
    }

    /**
     * @mixin MixinA
     * @mixin MixinB
     */
    class Test {}

    $test = new Test();

    $a = $test->a();
    $b = $test->b();

    assertType('string', $a);
    assertType('int', $b);
}

// Test: templatedMixinBindStatic
namespace PsalmTest_mixin_annotation_5 {
    /**
     * @template-covariant TModel of Model
     */
    class QueryBuilder {
        /**
         * @return list<TModel>
         */
        public function getInner() {
            return [];
        }
    }

    /**
     * @mixin QueryBuilder<static>
     */
    abstract class Model {}

    class FooModel extends Model {}

    $f = new FooModel();
    $g = $f->getInner();

    assertType('list<PsalmTest_mixin_annotation_5\FooModel>', $g);
}

// Test: mixinInheritMagicMethods
namespace PsalmTest_mixin_annotation_6 {
    /**
     * @method $this active()
     */
    class A {
        public function __call(string $name, array $arguments) {}
    }

    /**
     * @mixin A
     */
    class B {
        public function __call(string $name, array $arguments) {}
    }

    $b = new B;
    $c = $b->active();

    assertType('PsalmTest_mixin_annotation_6\B', $c);
}

