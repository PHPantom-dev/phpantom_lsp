<?php
// Source: Psalm Template/ClassTemplateExtendsTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: templateExtendsSameName
namespace PsalmTest_template_class_template_extends_1 {
    /**
     * @template TValue
     */
    class ValueContainer
    {
        /**
         * @var TValue
         */
        private $v;
        /**
         * @param TValue $v
         */
        public function __construct($v)
        {
            $this->v = $v;
        }
        /**
         * @return TValue
         */
        public function getValue()
        {
            return $this->v;
        }
    }

    /**
     * @template TKey
     * @template TValue
     * @template-extends ValueContainer<TValue>
     */
    class KeyValueContainer extends ValueContainer
    {
        /**
         * @var TKey
         */
        private $k;
        /**
         * @param TKey $k
         * @param TValue $v
         */
        public function __construct($k, $v)
        {
            $this->k = $k;
            parent::__construct($v);
        }
        /**
         * @return TKey
         */
        public function getKey()
        {
            return $this->k;
        }
    }
    $a = new KeyValueContainer("hello", 15);
    $b = $a->getValue();

    assertType('KeyValueContainer<string, int>', $a);
    assertType('int', $b);
}

// Test: templateExtendsDifferentName
namespace PsalmTest_template_class_template_extends_2 {
    /**
     * @template TValue
     */
    class ValueContainer
    {
        /**
         * @var TValue
         */
        private $v;
        /**
         * @param TValue $v
         */
        public function __construct($v)
        {
            $this->v = $v;
        }
        /**
         * @return TValue
         */
        public function getValue()
        {
            return $this->v;
        }
    }

    /**
     * @template TKey
     * @template Tv
     * @template-extends ValueContainer<Tv>
     */
    class KeyValueContainer extends ValueContainer
    {
        /**
         * @var TKey
         */
        private $k;
        /**
         * @param TKey $k
         * @param Tv $v
         */
        public function __construct($k, $v)
        {
            $this->k = $k;
            parent::__construct($v);
        }
        /**
         * @return TKey
         */
        public function getKey()
        {
            return $this->k;
        }
    }
    $a = new KeyValueContainer("hello", 15);
    $b = $a->getValue();

    assertType('KeyValueContainer<string, int>', $a);
    assertType('int', $b);
}

// Test: extendsWithNonTemplate
namespace PsalmTest_template_class_template_extends_3 {
    /**
     * @template T
     */
    abstract class Container
    {
        /**
         * @return T
         */
        public abstract function getItem();
    }

    class Foo
    {
    }

    /**
     * @template-extends Container<Foo>
     */
    class FooContainer extends Container
    {
        /**
         * @return Foo
         */
        public function getItem()
        {
            return new Foo();
        }
    }

    /**
     * @template TItem
     * @param Container<TItem> $c
     * @return TItem
     */
    function getItemFromContainer(Container $c) {
        return $c->getItem();
    }

    $fc = new FooContainer();

    $f1 = $fc->getItem();
    $f2 = getItemFromContainer($fc);

    assertType('FooContainer', $fc);
    assertType('Foo', $f1);
    assertType('Foo', $f2);
}

// Test: supportBareExtends
namespace PsalmTest_template_class_template_extends_4 {
    /**
     * @template T
     */
    abstract class Container
    {
        /**
         * @return T
         */
        public abstract function getItem();
    }

    class Foo
    {
    }

    /**
     * @extends Container<Foo>
     */
    class FooContainer extends Container
    {
        /**
         * @return Foo
         */
        public function getItem()
        {
            return new Foo();
        }
    }

    /**
     * @template TItem
     * @param Container<TItem> $c
     * @return TItem
     */
    function getItemFromContainer(Container $c) {
        return $c->getItem();
    }

    $fc = new FooContainer();

    $f1 = $fc->getItem();
    $f2 = getItemFromContainer($fc);

    assertType('FooContainer', $fc);
    assertType('Foo', $f1);
    assertType('Foo', $f2);
}

// Test: extendsWithNonTemplateWithoutImplementing
namespace PsalmTest_template_class_template_extends_5 {
    /**
     * @template T as array-key
     */
    abstract class User
    {
        /**
         * @var T
         */
        private $id;
        /**
         * @param T $id
         */
        public function __construct($id)
        {
            $this->id = $id;
        }
        /**
         * @return T
         */
        public function getID()
        {
            return $this->id;
        }
    }

    /**
     * @template-extends User<int>
     */
    class AppUser extends User {}

    $au = new AppUser(-1);
    $id = $au->getId();

    assertType('AppUser', $au);
    assertType('int', $id);
}

// Test: extendsTwiceSameNameCorrect
namespace PsalmTest_template_class_template_extends_6 {
    /**
     * @template T
     */
    class Container
    {
        /**
         * @var T
         */
        private $v;
        /**
         * @param T $v
         */
        public function __construct($v)
        {
            $this->v = $v;
        }
        /**
         * @return T
         */
        public function getValue()
        {
            return $this->v;
        }
    }

    /**
     * @template T
     * @template-extends Container<T>
     */
    class ChildContainer extends Container {}

    /**
     * @template T
     * @template-extends ChildContainer<T>
     */
    class GrandChildContainer extends ChildContainer {}

    $fc = new GrandChildContainer(5);
    $a = $fc->getValue();

    assertType('int', $a);
}

// Test: extendsTwiceDifferentNameUnbrokenChain
namespace PsalmTest_template_class_template_extends_7 {
    /**
     * @psalm-template T1
     */
    class Container
    {
        /**
         * @var T1
         */
        private $v;

        /**
         * @param T1 $v
         */
        public function __construct($v)
        {
            $this->v = $v;
        }

        /**
         * @return T1
         */
        public function getValue()
        {
            return $this->v;
        }
    }

    /**
     * @psalm-template T2
     * @extends Container<T2>
     */
    class ChildContainer extends Container {}

    /**
     * @psalm-template T3
     * @extends ChildContainer<T3>
     */
    class GrandChildContainer extends ChildContainer {}

    $fc = new GrandChildContainer(5);
    $a = $fc->getValue();

    assertType('int', $a);
}

// Test: templateExtendsOnceAndBound
namespace PsalmTest_template_class_template_extends_8 {
    /** @template T1 */
    class Repo {
        /** @return ?T1 */
        public function findOne() {
            return null;
        }
    }

    class SpecificEntity {}

    /** @template-extends Repo<SpecificEntity> */
    class AnotherRepo extends Repo {}

    $a = new AnotherRepo();
    $b = $a->findOne();

    assertType('AnotherRepo', $a);
    assertType('SpecificEntity|null', $b);
}

// Test: templateExtendsTwiceAndBound
namespace PsalmTest_template_class_template_extends_9 {
    /** @template T1 */
    class Repo {
        /** @return ?T1 */
        public function findOne() {
            return null;
        }
    }

    /**
     * @template T2
     * @template-extends Repo<T2>
     */
    class CommonAppRepo extends Repo {}

    class SpecificEntity {}

    /** @template-extends CommonAppRepo<SpecificEntity> */
    class SpecificRepo extends CommonAppRepo {}

    $a = new SpecificRepo();
    $b = $a->findOne();

    assertType('SpecificRepo', $a);
    assertType('SpecificEntity|null', $b);
}

// Test: templatedInterfaceExtendedMethodInheritReturnType
namespace PsalmTest_template_class_template_extends_10 {
    class Foo {}

    /**
     * @template-implements IteratorAggregate<int, Foo>
     */
    class SomeIterator implements IteratorAggregate
    {
        public function getIterator() {
            yield new Foo;
        }
    }

    $i = (new SomeIterator())->getIterator();

    // PHPantom is more precise than Psalm here: `getIterator()` is declared
    // `: Traversable`, so the `TValue[]` half of the stub's
    // `@return Traversable<TKey, TValue>|TValue[]` can never be returned.
    assertType('Traversable<int, Foo>', $i);
}

// Test: extendClassThatParameterizesTemplatedParent
namespace PsalmTest_template_class_template_extends_11 {
    /**
     * @template T
     */
    abstract class Collection
    {
        /**
         * @return array<T>
         */
        abstract function elements() : array;

        /**
         * @return T|null
         */
        public function first()
        {
            return $this->elements()[0] ?? null;
        }
    }

    /**
     * @template-extends Collection<int>
     */
    abstract class Bridge extends Collection {}


    class Service extends Bridge
    {
        /**
         * @return array<int>
         */
        public function elements(): array
        {
            return [1, 2, 3];
        }
    }

    $a = (new Service)->first();

    assertType('int|null', $a);
}

// Test: splObjectStorage
namespace PsalmTest_template_class_template_extends_12 {
    class SomeService
    {
        /**
         * @var \SplObjectStorage<\stdClass, mixed>
         */
        public $handlers;

        /**
         * @param SplObjectStorage<\stdClass, mixed> $handlers
         */
        public function __construct(SplObjectStorage $handlers)
        {
            $this->handlers = $handlers;
        }
    }

    /** @var SplObjectStorage<\stdClass, mixed> */
    $storage = new SplObjectStorage();
    new SomeService($storage);

    $c = new \stdClass();
    $storage[$c] = "hello";
    /** @psalm-suppress MixedAssignment */
    $b = $storage->offsetGet($c);

    assertType('mixed', $b);
}

// Test: templateExtendsOnceWithSpecificStaticCall
namespace PsalmTest_template_class_template_extends_13 {
    /**
     * @template T
     * @psalm-consistent-constructor
     * @psalm-consistent-templates
     */
    class Container {
        /** @var T */
        private $t;

        /** @param T $t */
        private function __construct($t) {
            $this->t = $t;
        }

        /**
         * @template U
         * @param U $t
         * @return static<U>
         */
        public static function getContainer($t) {
            return new static($t);
        }

        /**
         * @return T
         */
        public function getValue()
        {
            return $this->t;
        }
    }

    /**
     * @template T1
     * @template-extends Container<T1>
     */
    class AContainer extends Container {}

    class A {
        function foo() : void {}
    }

    $b = AContainer::getContainer(new A());

    assertType('AContainer<A>', $b);
}

// Test: templateExtendsDifferentNameWithStaticCall
namespace PsalmTest_template_class_template_extends_14 {
    /**
     * @template T
     * @psalm-consistent-constructor
     * @psalm-consistent-templates
     */
    class Container {
        /** @var T */
        private $t;

        /** @param T $t */
        private function __construct($t) {
            $this->t = $t;
        }

        /**
         * @template U
         * @param U $t
         * @return static<U>
         */
        public static function getContainer($t) {
            return new static($t);
        }

        /**
         * @return T
         */
        public function getValue()
        {
            return $this->t;
        }
    }

    /**
     * @template T1
     * @template-extends Container<T1>
     */
    class ObjectContainer extends Container {}

    /**
     * @template T2
     * @template-extends ObjectContainer<T2>
     */
    class AContainer extends ObjectContainer {}

    class A {
        function foo() : void {}
    }

    $b = AContainer::getContainer(new A());

    assertType('AContainer<A>', $b);
}

// Test: templateExtendsSameNameWithStaticCall
namespace PsalmTest_template_class_template_extends_15 {
    /**
     * @template T
     * @psalm-consistent-constructor
     * @psalm-consistent-templates
     */
    class Container {
        /** @var T */
        private $t;

        /** @param T $t */
        private function __construct($t) {
            $this->t = $t;
        }

        /**
         * @template U
         * @param U $t
         * @return static<U>
         */
        public static function getContainer($t) {
            return new static($t);
        }

        /**
         * @return T
         */
        public function getValue()
        {
            return $this->t;
        }
    }

    /**
     * @template T
     * @template-extends Container<T>
     */
    class ObjectContainer extends Container {}

    /**
     * @template T
     * @template-extends ObjectContainer<T>
     */
    class AContainer extends ObjectContainer {}

    class A {
        function foo() : void {}
    }

    $b = AContainer::getContainer(new A());

    assertType('AContainer<A>', $b);
}

// Test: extendArrayObjectWithTemplateParams
namespace PsalmTest_template_class_template_extends_16 {
    /**
     * @template TKey of array-key
     * @template TValue
     * @template-extends \ArrayObject<TKey,TValue>
     */
    class C extends \ArrayObject {
        /**
         * @param array<TKey,TValue> $kv
         */
        public function __construct(array $kv) {
            parent::__construct($kv);
        }
    }

    $c = new C(["a" => 1]);
    $i = $c->getIterator();

    assertType('C<string, int>', $c);
    assertType('ArrayIterator<string, int>', $i);
}

// Test: methodLevelTemplateKeyOfWithStringLiteral
namespace PsalmTest_template_class_template_extends_16b {
    /**
     * @template TData as array
     */
    class Config {
        /** @var TData */
        private $data;

        /** @param TData $data */
        public function __construct(array $data) {
            $this->data = $data;
        }

        /**
         * @template K as key-of<TData>
         * @param K $key
         * @return TData[K]
         */
        public function get(string $key) {
            return $this->data[$key];
        }
    }

    /** @var Config<array{name: string, age: int}> */
    $config = new Config(['name' => 'John', 'age' => 30]);

    $name = $config->get('name');
    $age = $config->get('age');

    assertType('string', $name);
    assertType('int', $age);
}

// Test: keyOfClassTemplateExtended
namespace PsalmTest_template_class_template_extends_17 {
    /**
     * @template TData as array
     * @psalm-no-seal-properties
     */
    abstract class DataBag {
        /**
         * @var TData
         */
        protected $data;

        /**
         * @param TData $data
         */
        public function __construct(array $data) {
            $this->data = $data;
        }

        /**
         * @template K as key-of<TData>
         *
         * @param K $property
         *
         * @return TData[K]
         */
        public function __get(string $property) {
            return $this->data[$property];
        }

        /**
         * @template K as key-of<TData>
         *
         * @param K $property
         * @param TData[K] $value
         */
        public function __set(string $property, $value) {
            $this->data[$property] = $value;
        }
    }

    /** @extends DataBag<array{a: int, b: string}> */
    class FooBag extends DataBag {}

    $foo = new FooBag(["a" => 5, "b" => "hello"]);

    $foo->a = 9;
    $foo->b = "hello";

    $a = $foo->a;
    $b = $foo->b;

    // The writes above dispatch to `__set`, which is free not to store
    // the value as given, so the reads resolve through `__get` instead
    // of through the written values.
    assertType('int', $a);
    assertType('string', $b);
}

// Test: inheritTemplateParamViaConstructorSameName
namespace PsalmTest_template_class_template_extends_18 {
    class Dog {}

    /**
     * @template T
     */
    class Collection {
        /** @var array<T> */
        protected $arr = [];

        /**
          * @param array<T> $arr
          */
        public function __construct(array $arr) {
            $this->arr = $arr;
        }
    }

    /**
     * @template T
     * @template V
     * @extends Collection<V>
     */
    class CollectionChild extends Collection {
    }

    $dogs = new CollectionChild([new Dog(), new Dog()]);

    assertType('CollectionChild<mixed, Dog>', $dogs);
}

// Test: inheritTemplateParamViaConstructorDifferentName
namespace PsalmTest_template_class_template_extends_19 {
    class Dog {}

    /**
     * @template T
     */
    class Collection {
        /** @var array<T> */
        protected $arr = [];

        /**
          * @param array<T> $arr
          */
        public function __construct(array $arr) {
            $this->arr = $arr;
        }
    }

    /**
     * @template U
     * @template V
     * @extends Collection<V>
     */
    class CollectionChild extends Collection {
    }

    $dogs = new CollectionChild([new Dog(), new Dog()]);

    assertType('CollectionChild<mixed, Dog>', $dogs);
}

// Test: implementsTemplatedTwice
namespace PsalmTest_template_class_template_extends_20 {
    /**
     * @template T1
     */
    interface A {
        /** @return T1 */
        public function get();
    }

    /**
     * @template T2
     * @extends A<T2>
     */
    interface B extends A {}

    /**
     * @template T3
     * @implements B<T3>
     */
    class C implements B {
        /** @var T3 */
        private $val;

        /**
         * @psalm-param T3 $val
         */
        public function __construct($val) {
            $this->val = $val;
        }

        public function get() {
            return $this->val;
        }
    }

    $foo = (new C("foo"))->get();

    assertType('string', $foo);
}

// Test: templateInheritedPropertyCorrectly
namespace PsalmTest_template_class_template_extends_21 {
    /**
     * @template TKey1
     * @template TValue1
     */
    class Pair
    {
        /** @psalm-var TKey1 */
        public $one;

        /** @psalm-var TValue1 */
        public $two;

        /**
         * @psalm-param TKey1 $key
         * @psalm-param TValue1 $value
         */
        public function __construct($key, $value) {
            $this->one = $key;
            $this->two = $value;
        }
    }

    /**
     * @template TValue2
     * @extends Pair<string, TValue2>
     */
    class StringKeyedPair extends Pair {
        /**
         * @param TValue2 $value
         */
        public function __construct(string $key, $value) {
            parent::__construct($key, $value);
        }
    }

    $pair = new StringKeyedPair("somekey", 250);
    $a = $pair->two;
    $b = $pair->one;

    assertType('StringKeyedPair<int>', $pair);
    assertType('int', $a);
    assertType('string', $b);
}

// Test: templateInheritedPropertySameName
namespace PsalmTest_template_class_template_extends_22 {
    /**
     * @template TKey
     * @template TValue
     */
    class Pair
    {
        /** @psalm-var TKey */
        public $one;

        /** @psalm-var TValue */
        public $two;

        /**
         * @psalm-param TKey $key
         * @psalm-param TValue $value
         */
        public function __construct($key, $value) {
            $this->one = $key;
            $this->two = $value;
        }
    }

    /**
     * @template TValue
     * @extends Pair<string, TValue>
     */
    class StringKeyedPair extends Pair {
        /**
         * @param TValue $value
         */
        public function __construct(string $key, $value) {
            parent::__construct($key, $value);
        }
    }

    $pair = new StringKeyedPair("somekey", 250);
    $a = $pair->two;
    $b = $pair->one;

    assertType('StringKeyedPair<int>', $pair);
    assertType('int', $a);
    assertType('string', $b);
}

// Test: templateInheritedPropertySameNameFlipped
namespace PsalmTest_template_class_template_extends_23 {
    /**
     * @template TKey
     * @template TValue
     */
    class Pair
    {
        /** @psalm-var TKey */
        public $one;

        /** @psalm-var TValue */
        public $two;

        /**
         * @psalm-param TKey $key
         * @psalm-param TValue $value
         */
        public function __construct($key, $value) {
            $this->one = $key;
            $this->two = $value;
        }
    }

    /**
     * @template TValue
     * @extends Pair<TValue, string>
     */
    class StringKeyedPair extends Pair {
        /**
         * @param TValue $value
         */
        public function __construct(string $key, $value) {
            parent::__construct($value, $key);
        }
    }

    $pair = new StringKeyedPair("somekey", 250);
    $a = $pair->one;
    $b = $pair->two;

    assertType('StringKeyedPair<int>', $pair);
    assertType('int', $a);
    assertType('string', $b);
}

// Test: templateExtendsFewerTemplateParameters
// Requires PHP 8.0
namespace PsalmTest_template_class_template_extends_24 {
    class Real {}

    class RealE extends Real {}

    /**
     * @template TKey as array-key
     * @template TValue as object
     */
    class a {
        /**
         * @param TKey $key
         * @param TValue $real
         */
        public function __construct(public int|string $key, public object $real) {}
        /**
         * @return TValue
         */
        public function ret(): object {
            return $this->real;
        }
    }
    /**
     * @template TTKey as array-key
     * @template TTValue as object
     *
     * @extends a<TTKey, TTValue>
     */
    class b extends a {
    }

    /**
     * @template TObject as Real
     *
     * @extends b<string, TObject>
     */
    class c1 extends b {
        /**
         * @param TObject $real
         */
        public function __construct(object $real) {
            parent::__construct("", $real);
        }
    }

    /**
     * @template TObject as Real
     * @template TOther
     *
     * @extends b<string, TObject>
     */
    class c2 extends b {
        /**
         * @param TOther $other
         * @param TObject $real
         */
        public function __construct($other, object $real) {
            parent::__construct("", $real);
        }
    }

    /**
     * @template TOther as object
     * @template TObject as Real
     *
     * @extends b<string, TObject|TOther>
     */
    class c3 extends b {
        /**
         * @param TOther $other
         * @param TObject $real
         */
        public function __construct(object $other, object $real) {
            parent::__construct("", $real);
        }
    }

    $a = new a(123, new RealE);
    $resultA = $a->ret();

    $b = new b(123, new RealE);
    $resultB = $b->ret();

    $c1 = new c1(new RealE);
    $resultC1 = $c1->ret();

    $c2 = new c2(false, new RealE);
    $resultC2 = $c2->ret();


    class Secondary {}

    $c3 = new c3(new Secondary, new RealE);
    $resultC3 = $c3->ret();

    assertType('a<int, RealE>', $a);
    assertType('RealE', $resultA);
    assertType('b<int, RealE>', $b);
    assertType('RealE', $resultB);
    assertType('c1<RealE>', $c1);
    assertType('RealE', $resultC1);
    assertType('c2<RealE, false>', $c2);
    assertType('RealE', $resultC2);
    assertType('c3<Secondary, RealE>', $c3);
    assertType('RealE|Secondary', $resultC3);
}

