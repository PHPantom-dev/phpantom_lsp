<?php
// Source: Psalm ClosureTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: byRefUseVar
namespace PsalmTest_closure_1 {
    $doNotContaminate = 123;

    $test = 123;

    $testBefore = $test;

    $testInsideBefore = null;
    $testInsideAfter = null;

    $v = function () use (&$test, &$testInsideBefore, &$testInsideAfter, $doNotContaminate): void {
        $testInsideBefore = $test;
        $test = "test";
        $testInsideAfter = $test;

        $doNotContaminate = "test";
    };

    assertType('123', $testBefore);
    assertType('\'test\'|123|null', $testInsideBefore);
    assertType('\'test\'|null', $testInsideAfter);
    assertType('\'test\'|123', $test);
    assertType('123', $doNotContaminate);
}

// Test: varReturnType
namespace PsalmTest_closure_2 {
    $add_one = function(int $a) : int {
        return $a + 1;
    };

    $a = $add_one(1);

    assertType('int', $a);
}

// Test: varReturnTypeArray
// Requires PHP 7.4
namespace PsalmTest_closure_3 {
    $add_one = fn(int $a) : int => $a + 1;

    $a = $add_one(1);

    assertType('int', $a);
}

// Test: arrayMapClosureVar
namespace PsalmTest_closure_4 {
    $mirror = function(int $i) : int { return $i; };
    $a = array_map($mirror, [1, 2, 3]);

    assertType('list{int, int, int}', $a);
}

// Test: returnsTypedClosureWithClasses
namespace PsalmTest_closure_5 {
    class A {}
    class B {}
    class C {}

    /**
     * @param Closure(B):A $f
     * @param Closure(C):B $g
     *
     * @return Closure(C):A
     */
    function foo(Closure $f, Closure $g) : Closure {
        return function (C $x) use ($f, $g) : A {
            return $f($g($x));
        };
    }

    $a = foo(
        function(B $b) : A { return new A;},
        function(C $c) : B { return new B;}
    )(new C);

    assertType('A', $a);
}

// Test: returnsTypedClosureWithSubclassParam
namespace PsalmTest_closure_6 {
    class A {}
    class B {}
    class C {}
    class C2 extends C {}

    /**
     * @param Closure(B):A $f
     * @param Closure(C):B $g
     *
     * @return Closure(C2):A
     */
    function foo(Closure $f, Closure $g) : Closure {
        return function (C $x) use ($f, $g) : A {
            return $f($g($x));
        };
    }

    $a = foo(
        function(B $b) : A { return new A;},
        function(C $c) : B { return new B;}
    )(new C2);

    assertType('A', $a);
}

// Test: returnsTypedClosureWithParentReturn
namespace PsalmTest_closure_7 {
    class A {}
    class B {}
    class C {}
    class A2 extends A {}

    /**
     * @param Closure(B):A2 $f
     * @param Closure(C):B $g
     *
     * @return Closure(C):A
     */
    function foo(Closure $f, Closure $g) : Closure {
        return function (C $x) use ($f, $g) : A2 {
            return $f($g($x));
        };
    }

    $a = foo(
        function(B $b) : A2 { return new A2;},
        function(C $c) : B { return new B;}
    )(new C);

    assertType('A', $a);
}

// Test: singleLineClosures
namespace PsalmTest_closure_8 {
    $a = function() : Closure { return function() : string { return "hello"; }; };
    $b = $a()();

    // PHPantom is more precise than Psalm here: the inner closure's body
    // returns the literal 'hello', narrower than its declared `: string`.
    assertType('"hello"', $b);
}

// Test: CallableWithArrayMap
namespace PsalmTest_closure_9 {
    /**
     * @psalm-template T
     * @param class-string<T> $className
     * @return callable(...mixed):T
     */
    function maker(string $className) {
       return function(...$args) use ($className) {
          /** @psalm-suppress MixedMethodCall */
          return new $className(...$args);
       };
    }
    $maker = maker(stdClass::class);
    $result = array_map($maker, ["abc"]);

    assertType('list{stdClass}', $result);
}

// Test: templateShenanigans
// Requires PHP 8.1
namespace PsalmTest_closure_10 {
    class inner {}
    class b {
        public inner $key;

        public function __construct() {
            $this->key = new inner;
        }
    }

    /**
     * @template-covariant TKey as array-key
     * @template TValue as b
     */
    class a {
        /**
         * @template TMappedValue
         *
         * @param (\Closure(TValue): TMappedValue)|true $callback Callback or null
         *
         * @return list<$callback is true ? array : TMappedValue>
         */
        public function toArray1(Closure|true $callback): array {
            return [];
        }
        /**
         * @template TMappedValue
         * @template T as (\Closure(TValue): TMappedValue)
         *
         * @param T $callback Callback or null
         *
         * @return list<TMappedValue>
         */
        public function toArray2(Closure $callback): array {
            return [];
        }
        /**
         * @template TMappedValue
         *
         * @param (\Closure(TValue): TMappedValue) $callback Callback or null
         *
         * @return list<TMappedValue>
         */
        public function toArray3(Closure $callback): array {
            return [];
        }
    }

    $a = (new a)->toArray1(static fn ($obj) => $obj->key);

    $b = (new a)->toArray2(static fn ($obj) => $obj->key);

    $c = (new a)->toArray3(static fn ($obj) => $obj->key);

    assertType('list<inner>', $a);
    assertType('list<inner>', $b);
    assertType('list<inner>', $c);
}

// Test: CallableWithArrayReduce
namespace PsalmTest_closure_11 {
    /**
     * @return callable(int, int): int
     */
    function maker() {
       return function(int $sum, int $e) {
          return $sum + $e;
       };
    }
    $maker = maker();
    $result = array_reduce([1, 2, 3], $maker, 0);

    assertType('int', $result);
}

// Test: FirstClassCallable:NamedFunction:is_int
// Requires PHP 8.1
namespace PsalmTest_closure_12 {
    $closure = is_int(...);
    $result = $closure(1);

    assertType('bool', $result);
}

// Test: FirstClassCallable:InstanceMethod:UserDefined
// Requires PHP 8.1
namespace PsalmTest_closure_13 {
    class Test {
        public function __construct(private readonly string $string) {
        }

        public function length(): int {
            return strlen($this->string);
        }
    }
    $test = new Test("test");
    $closure = $test->length(...);
    $length = $closure();

    assertType('int', $length);
}

// Test: FirstClassCallable:InstanceMethod:Expr
// Requires PHP 8.1
namespace PsalmTest_closure_14 {
    class Test {
        public function __construct(private readonly string $string) {
        }

        public function length(): int {
            return strlen($this->string);
        }
    }
    $test = new Test("test");
    $method_name = "length";
    $closure = $test->$method_name(...);
    $length = $closure();

    assertType('int', $length);
}

// Test: FirstClassCallable:InstanceMethod:BuiltIn
// Requires PHP 8.1
namespace PsalmTest_closure_15 {
    $queue = new \SplQueue;
    $closure = $queue->count(...);
    $count = $closure();

    // The stubs declare `SplQueue::count()` as `int<0, max>`.
    assertType('int<0, max>', $count);
}

// Test: FirstClassCallable:StaticMethod
// Requires PHP 8.1
namespace PsalmTest_closure_16 {
    class Test {
        public static function length(string $param): int {
            return strlen($param);
        }
    }
    $closure = Test::length(...);
    $length = $closure("test");

    assertType('int', $length);
}

// Test: FirstClassCallable:StaticMethod:Expr
// Requires PHP 8.1
namespace PsalmTest_closure_17 {
    class Test {
        public static function length(string $param): int {
            return strlen($param);
        }
    }
    $method_name = "length";
    $closure = Test::$method_name(...);
    $length = $closure("test");

    assertType('int', $length);
}

// Test: FirstClassCallable:InvokableObject
// Requires PHP 8.1
namespace PsalmTest_closure_18 {
    class Test {
        public function __invoke(string $param): int {
            return strlen($param);
        }
    }
    $test = new Test();
    $closure = $test(...);
    $length = $closure("test");

    assertType('int', $length);
}

// Test: FirstClassCallable:MagicInstanceMethod
// Requires PHP 8.1
namespace PsalmTest_closure_19 {
    /**
     * @method int length()
     */
    class Test {
        public function __construct(private readonly string $string) {
        }

        public function __call(string $name, array $args): mixed {
            return match ($name) {
                "length" => strlen($this->string),
                default => throw new \Error("Undefined method"),
            };
        }
    }
    $test = new Test("test");
    $closure = $test->length(...);
    $length = $closure();

    assertType('int', $length);
}

// Test: FirstClassCallable:MagicStaticMethod
// Requires PHP 8.1
namespace PsalmTest_closure_20 {
    /**
     * @method static int length(string $length)
     */
    class Test {
        public static function __callStatic(string $name, array $args): mixed {
            return match ($name) {
                "length" => strlen((string) $args[0]),
                default => throw new \Error("Undefined method"),
            };
        }
    }
    $closure = Test::length(...);
    $length = $closure("test");

    assertType('int', $length);
}

// Test: FirstClassCallable:WithArrayMap
// Requires PHP 8.1
namespace PsalmTest_closure_21 {
    $array = [1, 2, 3];
    $closure = fn (int $value): int => $value * $value;
    $result1 = array_map((new \SplQueue())->enqueue(...), $array);
    $result2 = array_map(strval(...), $array);
    $result3 = array_map($closure(...), $array);

    assertType('list{null, null, null}', $result1);
    assertType('list{string, string, string}', $result2);
    assertType('list{int, int, int}', $result3);
}

// Test: FirstClassCallable:AssignmentVisitorMap
// Requires PHP 8.1
namespace PsalmTest_closure_22 {
    class Test {
        /** @var list<\Closure():void> */
        public array $handlers = [];

        public function register(): void {
            foreach ([1, 2, 3] as $index) {
                $this->push($this->handler(...));
            }
        }

        /**
         * @param Closure():void $closure
         * @return void
         */
        private function push(\Closure $closure): void {
            $this->handlers[] = $closure;
        }

        private function handler(): void {
        }
    }

    $test = new Test();
    $test->register();
    $handlers = $test->handlers;

    assertType('list<Closure():void>', $handlers);
}

// Test: FirstClassCallable:Method:Asserted
// Requires PHP 8.1
namespace PsalmTest_closure_23 {
    $r = false;
    /** @var object $o */;
    /** @var string $m */;
    if (method_exists($o, $m)) {
        $r = $o->$m(...);
    }

    assertType('Closure|false', $r);
}

