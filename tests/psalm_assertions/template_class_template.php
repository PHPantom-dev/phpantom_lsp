<?php
// Source: Psalm Template/ClassTemplateTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: cachingIterator
namespace PsalmTest_template_class_template_1 {
    $input = range("a", "z");

    $arrayIterator = new ArrayIterator($input);
    $decoratorIterator = new CachingIterator($arrayIterator);
    $next = $decoratorIterator->hasNext();
    $key = $decoratorIterator->key();
    $value = $decoratorIterator->current();

    assertType('string', $value);
    assertType('bool', $next);
}

// Test: infiniteIterator
namespace PsalmTest_template_class_template_2 {
    $input = range("a", "z");

    $arrayIterator = new ArrayIterator($input);
    $decoratorIterator = new InfiniteIterator($arrayIterator);
    $key = $decoratorIterator->key();
    $value = $decoratorIterator->current();

    assertType('string', $value);
}

// Test: limitIterator
namespace PsalmTest_template_class_template_3 {
    $input = range("a", "z");

    $arrayIterator = new ArrayIterator($input);
    $decoratorIterator = new LimitIterator($arrayIterator, 1, 1);
    $key = $decoratorIterator->key();
    $value = $decoratorIterator->current();

    assertType('string', $value);
}

// Test: callbackFilterIterator
namespace PsalmTest_template_class_template_4 {
    $input = range("a", "z");

    $arrayIterator = new ArrayIterator($input);
    $decoratorIterator = new CallbackFilterIterator(
        $arrayIterator,
        static function (string $value): bool {return "a" === $value;}
    );
    $key = $decoratorIterator->key();
    $value = $decoratorIterator->current();

    assertType('string', $value);
}

// Test: noRewindIterator
namespace PsalmTest_template_class_template_5 {
    $input = range("a", "z");

    $arrayIterator = new ArrayIterator($input);
    $decoratorIterator = new NoRewindIterator($arrayIterator);
    $key = $decoratorIterator->key();
    $value = $decoratorIterator->current();

    assertType('string', $value);
}


// Test: classTemplate
namespace PsalmTest_template_class_template_6 {
    class A {}
    class B {}
    class C {}
    class D {}

    /**
     * @template T as object
     */
    class Foo {
        /** @var T::class */
        public $T;

        /**
         * @param class-string<T> $T
         */
        public function __construct(string $T) {
            $this->T = $T;
        }

        /**
         * @return T
         * @psalm-suppress MixedMethodCall
         */
        public function bar() {
            $t = $this->T;
            return new $t();
        }
    }

    $at = "A";

    /**
     * @var Foo<A>
     * @psalm-suppress ArgumentTypeCoercion
     */
    $afoo = new Foo($at);
    $afoo_bar = $afoo->bar();

    $bfoo = new Foo(B::class);
    $bfoo_bar = $bfoo->bar();

    // this shouldn’t cause a problem as it’s a docbblock type
    if (!($bfoo_bar instanceof B)) {}

    $c = C::class;
    $cfoo = new Foo($c);
    $cfoo_bar = $cfoo->bar();

    assertType('Foo<A>', $afoo);
    assertType('A', $afoo_bar); 
    assertType('Foo<B>', $bfoo);
    assertType('B', $bfoo_bar);
    assertType('Foo<C>', $cfoo);
    assertType('C', $cfoo_bar);
}

// Test: classTemplateSelf
namespace PsalmTest_template_class_template_7 {
    /**
     * @template T as object
     */
    class Foo {
        /** @var class-string<T> */
        public $T;

        /**
         * @param class-string<T> $T
         */
        public function __construct(string $T) {
            $this->T = $T;
        }

        /**
         * @return T
         * @psalm-suppress MixedMethodCall
         */
        public function bar() {
            $t = $this->T;
            return new $t();
        }
    }

    class E {
        /**
         * @return Foo<self>
         */
        public static function getFoo() {
            return new Foo(__CLASS__);
        }

        /**
         * @return Foo<self>
         */
        public static function getFoo2() {
            return new Foo(self::class);
        }

        /**
         * @return Foo<static>
         */
        public static function getFoo3() {
            return new Foo(static::class);
        }
    }

    class G extends E {}

    $efoo = E::getFoo();
    $efoo2 = E::getFoo2();
    $efoo3 = E::getFoo3();

    $gfoo = G::getFoo();
    $gfoo2 = G::getFoo2();
    $gfoo3 = G::getFoo3();

    assertType('Foo<E>', $efoo);
    assertType('Foo<E>', $efoo2);
    assertType('Foo<PsalmTest_template_class_template_7\E>', $efoo3);
    assertType('Foo<E>', $gfoo);
    assertType('Foo<E>', $gfoo2);
    assertType('Foo<PsalmTest_template_class_template_7\G>', $gfoo3);
}

// Test: classTemplateExternalClasses
namespace PsalmTest_template_class_template_8 {
    /**
     * @template T as object
     */
    class Foo {
        /** @var T::class */
        public $T;

        /**
         * @param class-string<T> $T
         */
        public function __construct(string $T) {
            $this->T = $T;
        }

        /**
         * @return T
         * @psalm-suppress MixedMethodCall
         */
        public function bar() {
            $t = $this->T;
            return new $t();
        }
    }

    $efoo = new Foo(\Exception::class);
    $efoo_bar = $efoo->bar();

    $ffoo = new Foo(\LogicException::class);
    $ffoo_bar = $ffoo->bar();

    assertType('Foo<Exception>', $efoo);
    assertType('Exception', $efoo_bar);
    assertType('Foo<LogicException>', $ffoo);
    assertType('LogicException', $ffoo_bar);
}

// Test: classTemplateContainerSimpleCall
namespace PsalmTest_template_class_template_9 {
    class A {}

    /**
     * @template T
     */
    class Foo {
        /** @var T */
        public $obj;

        /**
         * @param T $obj
         */
        public function __construct($obj) {
            $this->obj = $obj;
        }

        /**
         * @return T
         */
        public function bar() {
            return $this->obj;
        }
    }

    $afoo = new Foo(new A());
    $afoo_bar = $afoo->bar();

    assertType('Foo<A>', $afoo);
    assertType('A', $afoo_bar);
}

// Test: getMagicPropertyOnClass
namespace PsalmTest_template_class_template_10 {
    class A {}

    /**
     * @template T as A
     * @property ?T $x
     */
    class B {
        /** @var ?T */
        public $y;

        public function __get() {}
    }

    $b = new B();
    $b_x = $b->x;
    $b_y = $b->y;

    assertType('A|null', $b_x);
    assertType('A|null', $b_y);
}

// Test: mixedTemplatedParamOutWithNoExtendedTemplate
namespace PsalmTest_template_class_template_11 {
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
     * @psalm-suppress MissingTemplateParam
     * @template TKey
     * @template TValue
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
    assertType('mixed', $b);
}

// Test: mixedTemplatedParamOutDifferentParamName
namespace PsalmTest_template_class_template_12 {
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
     *
     * @psalm-suppress MissingTemplateParam
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
    assertType('mixed', $b);
}

// Test: doesntExtendTemplateAndDoesNotOverride
namespace PsalmTest_template_class_template_13 {
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
     * @psalm-suppress MissingTemplateParam
     */
    class AppUser extends User {}

    $au = new AppUser(-1);
    $id = $au->getId();

    assertType('AppUser', $au);
    assertType('array-key', $id);
}

// Test: templateTKeyedArrayValues
namespace PsalmTest_template_class_template_14 {
    /**
     * @template TKey
     * @template TValue
     */
    class Collection {
        /**
         * @return array{0:Collection<TKey,TValue>,1:Collection<TKey,TValue>}
         * @psalm-suppress InvalidReturnType
         */
        public function partition() {}
    }

    /** @var Collection<int,string> $c */
    $c = new Collection;
    [$partA, $partB] = $c->partition();

    assertType('Collection<int, string>', $partA);
    assertType('Collection<int, string>', $partB);
}

// Test: doublyLinkedListConstructor
namespace PsalmTest_template_class_template_15 {
    $list = new SplDoublyLinkedList();
    $list->add(5, "hello");
    $list->add(5, 1);

    /** @var SplDoublyLinkedList<string> */
    $templated_list = new SplDoublyLinkedList();
    $templated_list->add(5, "hello");
    $a = $templated_list->bottom();

    assertType('string', $a);
}

// Test: allowTemplateParamsToCoerceToMinimumTypes
namespace PsalmTest_template_class_template_16 {
    /**
     * @psalm-template TKey of array-key
     * @psalm-template T
     */
    class ArrayCollection
    {
        /**
         * @var array<TKey,T>
         */
        private $elements;

        /**
         * @param array<TKey,T> $elements
         */
        public function __construct(array $elements = [])
        {
            $this->elements = $elements;
        }
    }

    /** @psalm-suppress MixedArgument */
    $c = new ArrayCollection($GLOBALS["a"]);

    assertType('ArrayCollection<array-key, mixed>', $c);
}

// Test: doNotCombineTypes
namespace PsalmTest_template_class_template_17 {
    class A {}
    class B {}

    /**
     * @template T
     */
    class C {
        /**
         * @var T
         */
        private $t;

        /**
         * @param T $t
         */
        public function __construct($t) {
            $this->t = $t;
        }

        /**
         * @return T
         */
        public function get() {
            return $this->t;
        }
    }

    /**
     * @param C<A> $a
     * @param C<B> $b
     * @return C<A>|C<B>
     */
    function randomCollection(C $a, C $b) : C {
        if (rand(0, 1)) {
            return $a;
        }

        return $b;
    }

    $random_collection = randomCollection(new C(new A), new C(new B));

    $a_or_b = $random_collection->get();

    assertType('C<A>|C<B>', $random_collection);
    assertType('A|B', $a_or_b);
}

// Test: doNotCombineTypesWhenMemoized
namespace PsalmTest_template_class_template_18 {
    class A {}
    class B {}

    /**
     * @template T
     */
    class C {
        /**
         * @var T
         */
        private $t;

        /**
         * @param T $t
         */
        public function __construct($t) {
            $this->t = $t;
        }

        /**
         * @return T
         * @psalm-mutation-free
         */
        public function get() {
            return $this->t;
        }
    }

    /** @var C<A>|C<B> $random_collection **/
    $a_or_b = $random_collection->get();

    assertType('C<A>|C<B>', $random_collection);
    assertType('A|B', $a_or_b);
}

// Test: templatedGet
namespace PsalmTest_template_class_template_19 {
    /**
     * @template P as string
     * @template V as mixed
     * 
     * @psalm-no-seal-properties
     */
    class PropertyBag {
        /** @var array<P,V> */
        protected $data = [];

        /** @param array<P,V> $data */
        public function __construct(array $data) {
            $this->data = $data;
        }

        /** @param P $name */
        public function __isset(string $name): bool {
            return isset($this->data[$name]);
        }

        /**
         * @param P $name
         * @return V
         */
        public function __get(string $name) {
            return $this->data[$name];
        }
    }

    $p = new PropertyBag(["a" => "data for a", "b" => "data for b"]);

    $a = $p->a;

    assertType('string', $a);
}

// Test: templatedInterfaceIntersectionSecond
namespace PsalmTest_template_class_template_20 {
    /** @psalm-template T */
    interface IParent {
        /** @psalm-return T */
        function foo();
    }

    /** @psalm-suppress MissingTemplateParam */
    interface IChild extends IParent {}

    class C {}

    /** @psalm-return IChild&IParent<C> */
    function makeConcrete() : IChild {
        return new class() implements IChild {
            public function foo() {
                return new C();
            }
        };
    }

    $a = makeConcrete()->foo();

    assertType('C', $a);
}

// Test: returnTemplateIntersectionGenericObjectAndTemplate
namespace PsalmTest_template_class_template_21 {
    /** @psalm-template Tp */
    interface I {
        /** @psalm-return Tp */
        function getMe();
    }

    class C {}

    /**
     * @psalm-template T as object
     *
     * @psalm-param class-string<T> $className
     *
     * @psalm-return T&I<T>
     *
     * @psalm-suppress MissingTemplateParam
     */
    function makeConcrete(string $className) : object
    {
        /** @var T&I<T> */
        return new class() extends C implements I {
            public function getMe() {
                return $this;
            }
        };
    }

    $a = makeConcrete(C::class);

    assertType('C&I<C>', $a);
}

// Test: weakReferenceIsTyped
namespace PsalmTest_template_class_template_22 {
    $e = new Exception;
    $r = WeakReference::create($e);
    $ex = $r->get();

    assertType('Exception|null', $ex);
}

// Test: createEmptyArrayCollection
namespace PsalmTest_template_class_template_23 {
    $a = new ArrayCollection([]);

    /**
     * @psalm-template TKey of array-key
     * @psalm-template T
     */
    class ArrayCollection
    {
        /**
         * An array containing the entries of this collection.
         *
         * @psalm-var array<TKey,T>
         * @var array
         */
        private $elements = [];

        /**
         * Initializes a new ArrayCollection.
         *
         * @param array $elements
         *
         * @psalm-param array<TKey,T> $elements
         */
        public function __construct(array $elements = [])
        {
            $this->elements = $elements;
        }

        /**
         * @param TKey $key
         * @param T $t
         */
        public function add($key, $t) : void {
            $this->elements[$key] = $t;
        }
    }

    assertType('ArrayCollection<never, never>', $a);
}

// Test: unionClassStringInferenceAndDefaultEmptyArray
namespace PsalmTest_template_class_template_24 {
    class A{}

    $packages = Collection::fromClassString(A::class);

    /**
     * @template T
     */
    class Collection{
        /** @var array<T> $items */
        protected $items = [];

        /**
         * @param array<string, T> $items
         */
        public function __construct(array $items = [])
        {
            $this->items = $items;
        }

        /**
         * @template C as object
         * @param class-string<C> $classString
         * @param array<string, C> $elements
         * @return Collection<C>
         */
        public static function fromClassString(string $classString, array $elements = []) : Collection
        {
            return new Collection($elements);
        }
    }

    assertType('Collection<A>', $packages);
}

// Test: newWithoutInferredTemplate
namespace PsalmTest_template_class_template_25 {
    /**
     * @psalm-template T2 of object
     */
    final class Foo {}

    $f = new Foo();

    assertType('Foo<object>', $f);
}

// Test: templateDefaultSimpleString
namespace PsalmTest_template_class_template_26 {
    /**
     * @template T as string
     */
    class C {
        /** @var T */
        public $t;

        /**
         * @param T $t
         */
        function __construct(string $t = "hello") {
            $this->t = $t;
        }
    }

    $c = new C();

    // PHPantom, like PHPStan, widens a literal argument when inferring a class template.
    assertType('C<string>', $c);
}

// Test: templateDefaultClassConstant
namespace PsalmTest_template_class_template_27 {
    class D {}

    /**
     * @template T as object
     */
    class E {
        /** @var class-string<T> */
        public $t;

        /**
         * @param class-string<T> $t
         */
        function __construct(string $t = D::class) {
            $this->t = $t;
        }
    }

    $e = new E();

    assertType('E<D>', $e);
}

// Test: combineTwoTemplatedArrays
namespace PsalmTest_template_class_template_28 {
    /** @template T */
    class Option
    {
        /** @param T $v */
        public function __construct(private $v) {}

        /**
         * @template E
         * @param E $else
         * @return T|E
         */
        public function getOrElse($else)
        {
           return rand(0, 1) === 1 ? $this->v : $else;
        }
    }

    $opt = new Option([1, 3]);

    $b = $opt->getOrElse([2, 4])[0];

    assertType('1|2', $b);
}

// Test: complexTypes
namespace PsalmTest_template_class_template_29 {
    /**
     * @template T
     */
    class Future {
        /**
         * @param T $v
         */
        public function __construct(private $v) {}
        /** @return T */
        public function get() { return $this->v; }
    }


    /**
     * @template TTObject
     *
     * @extends Future<ArrayObject<int, TTObject>>
     */
    class FutureB extends Future {
        /** @param TTObject $data */
        public function __construct($data) { parent::__construct(new ArrayObject([$data])); }
    }

    $a = new FutureB(123);

    $r = $a->get();

    // PHPantom, like PHPStan, widens a literal argument when inferring a class template.
    assertType('FutureB<int>', $a);
    assertType('ArrayObject<int, int>', $r);
}

// Test: promoted property with template
// Requires PHP 8.0
namespace PsalmTest_template_class_template_30 {
    /**
     * @template T
     */
    class A {
        public function __construct(
            /** @var T */
            public mixed $t
        ) {}
    }

    $a = new A(5);
    $t = $a->t;

    assertType('A<int>', $a);
    assertType('int', $t);
}

// Test: template of simple type with additional comment without dot
namespace PsalmTest_template_class_template_31 {
    /**
     * @psalm-template T of string
     *
     * lorem ipsum
     */
    class Foo {
        /** @psalm-var T */
        public string $t;

        /** @psalm-param T $t */
        public function __construct(string $t) {
            $this->t = $t;
        }

        /**
         * @psalm-return T
         */
        public function t(): string {
            return $this->t;
        }
    }
    $t = (new Foo(''))->t();

    // PHPantom, like PHPStan, widens a literal argument when inferring a class template.
    assertType('string', $t);
}

// Test: template of simple type with additional comment with dot
namespace PsalmTest_template_class_template_32 {
    /**
     * @psalm-template T of string
     *
     * lorem ipsum.
     */
    class Foo {
        /** @psalm-var T */
        public string $t;

        /** @psalm-param T $t */
        public function __construct(string $t) {
            $this->t = $t;
        }

        /**
         * @psalm-return T
         */
        public function t(): string {
            return $this->t;
        }
    }
    $t = (new Foo(''))->t();

    // PHPantom, like PHPStan, widens a literal argument when inferring a class template.
    assertType('string', $t);
}
