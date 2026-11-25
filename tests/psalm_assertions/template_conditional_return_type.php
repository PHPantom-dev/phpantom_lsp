<?php
// Source: Psalm Template/ConditionalReturnTypeTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: conditionalReturnTypeSimple
namespace PsalmTest_template_conditional_return_type_1 {
    class A {
        /** @var array<string, string> */
        private array $itemAttr = [];

        /**
         * @template T as ?string
         * @param T $name
         * @return string|string[]
         * @psalm-return (T is string ? string : array<string, string>)
         */
        public function getAttribute(?string $name, string $default = "")
        {
            if (null === $name) {
                return $this->itemAttr;
            }
            return isset($this->itemAttr[$name]) ? $this->itemAttr[$name] : $default;
        }
    }

    $a = (new A)->getAttribute("colour", "red"); // typed as string
    $b = (new A)->getAttribute(null); // typed as array<string, string>
    /** @psalm-suppress MixedArgument */
    $c = (new A)->getAttribute($GLOBALS["foo"]); // typed as string|array<string, string>

    assertType('string', $a);
    assertType('array<string, string>', $b);
    assertType('array<string, string>|string', $c);
}

// Test: userlandAddition
namespace PsalmTest_template_conditional_return_type_2 {
    /**
     * @template T as int|float
     * @param T $a
     * @param T $b
     * @return int|float
     * @psalm-return (T is int ? int : float)
     */
    function add($a, $b) {
        return $a + $b;
    }

    $int = add(3, 5);
    $float1 = add(2.5, 3);
    $float2 = add(2.7, 3.1);
    $float3 = add(3, 3.5);
    /** @psalm-suppress PossiblyNullArgument */
    $int = add(rand(0, 1) ? null : 1, 1);

    assertType('int', $int);
    assertType('float|int', $float1);
    assertType('float', $float2);
    assertType('float|int', $float3);
}

// Test: possiblyNullArgumentStillMatchesType
namespace PsalmTest_template_conditional_return_type_3 {
    /**
     * @template T as int|float
     * @param T $a
     * @param T $b
     * @return int|float
     * @psalm-return (T is int ? int : float)
     */
    function add($a, $b) {
        return $a + $b;
    }

    /** @psalm-suppress PossiblyNullArgument */
    $int = add(rand(0, 1) ? null : 1, 4);

    assertType('int', $int);
}

// Test: nestedClassConstantConditionalComparison
namespace PsalmTest_template_conditional_return_type_4 {
    class A {
        const TYPE_STRING = 0;
        const TYPE_INT = 1;

        /**
         * @template T as int
         * @param T $i
         * @psalm-return (
         *     T is self::TYPE_STRING
         *     ? string
         *     : (T is self::TYPE_INT ? int : bool)
         * )
         */
        public static function getDifferentType(int $i) {
            if ($i === self::TYPE_STRING) {
                return "hello";
            }

            if ($i === self::TYPE_INT) {
                return 5;
            }

            return true;
        }
    }

    $string = A::getDifferentType(0);
    $int = A::getDifferentType(1);
    $bool = A::getDifferentType(4);
    $string2 = (new A)->getDifferentType(0);
    $int2 = (new A)->getDifferentType(1);
    $bool2 = (new A)->getDifferentType(4);

    assertType('string', $string);
    assertType('int', $int);
    assertType('bool', $bool);
    assertType('string', $string2);
    assertType('int', $int2);
    assertType('bool', $bool2);
}

// Test: refineTypeInConditionalWithString
namespace PsalmTest_template_conditional_return_type_5 {
    /**
     * @template TInput
     *
     * @param TInput $input
     *
     * @return (TInput is string ? TInput : 'hello')
     */
    function foobaz($input): string {
        if (is_string($input)) {
            return $input;
        }

        return "hello";
    }

    $a = foobaz("boop");
    $b = foobaz(4);

    // PHPantom keeps the literal values Psalm's assertion widens to their base type.
    assertType('"boop"', $a);
    assertType('\'hello\'', $b);
}

// Test: refineTypeInConditionalWithClassName
namespace PsalmTest_template_conditional_return_type_6 {
    class A {}
    class AChild extends A {}
    class B {}

    /**
     * @template TInput as object
     *
     * @param TInput $input
     *
     * @return (TInput is A ? TInput : A)
     */
    function foobaz(object $input): A {
        if ($input instanceof A) {
            return $input;
        }

        return new A();
    }

    $a = foobaz(new AChild());
    $b = foobaz(new B());

    assertType('AChild', $a);
    assertType('A', $b);
}

// Test: promiseConditional
namespace PsalmTest_template_conditional_return_type_7 {
    /**
     * @template T
     */
    class Promise {
        /** @var T */
        private $t;

        /** @param T $t */
        public function __construct($t) {
            $this->t = $t;
        }
    }

    /**
     * @template T
     * @extends Promise<T>
     */
    class Success extends Promise {}

    /**
     * @template TReturn
     * @template TPromise
     *
     * @template T as Promise<TPromise>|TReturn
     *
     * @param callable(): T $callback
     *
     * @return Promise
     * @psalm-return (T is Promise ? Promise<TPromise> : Promise<TReturn>)
     */
    function call(callable $callback): Promise {
        $result = $callback();

        if ($result instanceof Promise) {
            return $result;
        }

        return new Promise($result);
    }

    $ret_int_promise = function (): Promise {
        return new Success(9);
    };

    $c1 = call($ret_int_promise);

    $c2 = call(function (): int {
        return 42;
    });

    assertType('Promise<int>', $c1);
    assertType('Promise<int>', $c2); // SKIP: a conditional on a callable argument's return type keeps both branches
}

// Test: conditionalOnArgCount
namespace PsalmTest_template_conditional_return_type_8 {
    /**
     * @return (func_num_args() is 0 ? false : string)
     */
    function zeroArgsFalseOneArgString(string $s = "") {
        if (func_num_args() === 0) {
            return false;
        }

        return $s;
    }

    $a = zeroArgsFalseOneArgString();
    $b = zeroArgsFalseOneArgString("");
    $c = zeroArgsFalseOneArgString("hello");

    assertType('string', $b);
    assertType('string', $c);
}

// Test: stringOrClassStringT
namespace PsalmTest_template_conditional_return_type_9 {
    class A {}

    /**
     * @template T
     * @param literal-string|class-string<T> $name
     * @return ($name is class-string ? T : mixed)
     */
    function get(string $name) {
        return;
    }

    $lowercase_a = "a";

    /** @var class-string $class_string */
    $class_string = "b";

    /** @psalm-suppress MixedAssignment */
    $expect_mixed = get($lowercase_a);
    $expect_object = get($class_string);

    $expect_a_object = get(A::class);

    /** @psalm-suppress MixedAssignment */
    $expect_mixed_from_literal = get("LiteralDirect");

    assertType('mixed', $expect_mixed);
    assertType('object', $expect_object); // SKIP: a bare `class-string` argument leaves `class-string<T>`'s template unbound
    assertType('A', $expect_a_object);
    assertType('mixed', $expect_mixed_from_literal);
}

// Test: returnTypeBasedOnPhpVersionId
// Requires PHP 7.2
namespace PsalmTest_template_conditional_return_type_10 {
    /**
     * @psalm-return (PHP_VERSION_ID is int<70300, max> ? string : int)
     */
    function getSomething()
    {
        return mt_rand(1, 10) > 5 ? "a value" : 42;
    }

    /**
     * @psalm-return (PHP_VERSION_ID is int<70100, max> ? string : int)
     */
    function getSomethingElse()
    {
        return mt_rand(1, 10) > 5 ? "a value" : 42;
    }

    $something = getSomething();
    $somethingElse = getSomethingElse();

    // Psalm runs this case as PHP 7.2; at PHP 8.5 `PHP_VERSION_ID` is above 70300.
    assertType('string', $something);
    assertType('string', $somethingElse);
}

// Test: ineritedreturnTypeBasedOnPhpVersionId
// Requires PHP 7.2
namespace PsalmTest_template_conditional_return_type_11 {
    class A {
    /**
     * @psalm-return (PHP_VERSION_ID is int<70300, max> ? string : int)
     */
    function getSomething()
    {
        return mt_rand(1, 10) > 5 ? "a value" : 42;
    }

    /**
     * @psalm-return (PHP_VERSION_ID is int<70100, max> ? string : int)
     */
    function getSomethingElse()
    {
        return mt_rand(1, 10) > 5 ? "a value" : 42;
    }
    }

    class B extends A {}

    $class = new B();
    $something = $class->getSomething();
    $somethingElse = $class->getSomethingElse();

    // Psalm runs this case as PHP 7.2; at PHP 8.5 `PHP_VERSION_ID` is above 70300.
    assertType('string', $something);
    assertType('string', $somethingElse);
}

// Test: nonEmptyLiteralString
namespace PsalmTest_template_conditional_return_type_12 {
    /**
     * @param literal-string $string
     * @psalm-return ($string is non-empty-literal-string ? string : int)
     */
    function getSomething(string $string)
    {
        if (!$string) {
            return 1;
        }

        return "";
    }

    /** @var literal-string $literalString */
    $literalString;
    $something = getSomething($literalString);
    /** @var non-empty-literal-string $nonEmptyliteralString */
    $nonEmptyliteralString;
    $something2 = getSomething($nonEmptyliteralString);

    assertType('int|string', $something);
    assertType('string', $something2);
}

// Test: lowercaseStringIsNotNonEmptyWithUnion
namespace PsalmTest_template_conditional_return_type_13 {
    /**
     * @param string|int $stringOrInt
     * @psalm-return ($stringOrInt is (non-empty-string)|int ? string : int)
     */
    function getSomething($stringOrInt)
    {
        if (!$stringOrInt) {
            return 1;
        }
        return "";
    }
    /** @var string $string */
    $string;
    $something = getSomething($string);
    /** @var non-empty-string $nonEmptyString */
    $nonEmptyString;
    $something2 = getSomething($nonEmptyString);

    assertType('int|string', $something);
    assertType('string', $something2);
}

// Test: literalStringIsNotNonEmpty
namespace PsalmTest_template_conditional_return_type_14 {
    /**
     * @param literal-string $literalString
     * @psalm-return ($literalString is non-empty-string ? string : int)
     */
    function getSomething(string $literalString)
    {
        if (!$literalString) {
            return 1;
        }
        return "";
    }
    /** @var literal-string $literalString */
    $literalString;
    $something = getSomething($literalString);
    /** @var non-empty-literal-string $nonEmptyliteralString */
    $nonEmptyliteralString;
    $something2 = getSomething($nonEmptyliteralString);

    assertType('int|string', $something);
    assertType('string', $something2);
}

// Test: literalStringIsNotNonEmptyWithUnion
namespace PsalmTest_template_conditional_return_type_15 {
    /**
     * @param literal-string|int $literalStringOrInt
     * @psalm-return ($literalStringOrInt is non-empty-string|int ? string : int)
     */
    function getSomething($literalStringOrInt)
    {
        if (!$literalStringOrInt) {
            return 1;
        }
        return "";
    }
    /** @var literal-string $literalString */
    $literalString;
    $something = getSomething($literalString);
    /** @var non-empty-literal-string $nonEmptyliteralString */
    $nonEmptyliteralString;
    $something2 = getSomething($nonEmptyliteralString);

    assertType('int|string', $something);
    assertType('string', $something2);
}

// Test: lowercaseStringAndNotEmpty
namespace PsalmTest_template_conditional_return_type_16 {
    /**
     * @param lowercase-string $string
     * @psalm-return ($string is non-empty-string ? string : int)
     */
    function lowercaseIsNonEmpty($string)
    {
        if (!$string) {
            return 1;
        }
        return "";
    }
    /**
     * @param lowercase-string $string
     * @psalm-return ($string is non-empty-lowercase-string ? string : int)
     */
    function lowercaseIsNonEmptyLowercase($string)
    {
        if (!$string) {
            return 1;
        }
        return "";
    }

    /** @var lowercase-string $lowercaseString */
    $lowercaseString;
    /** @var non-empty-lowercase-string $nonEmptyLowercaseString */
    $nonEmptyLowercaseString;

    $lowercaseString1 = lowercaseIsNonEmpty($lowercaseString);
    $lowercaseString2 = lowercaseIsNonEmptyLowercase($lowercaseString);

    $nonEmptyLowercaseString1 = lowercaseIsNonEmpty($nonEmptyLowercaseString);
    $nonEmptyLowercaseString2 = lowercaseIsNonEmptyLowercase($nonEmptyLowercaseString);

    assertType('int|string', $lowercaseString1);
    assertType('int|string', $lowercaseString2);
    assertType('string', $nonEmptyLowercaseString1);
    assertType('string', $nonEmptyLowercaseString2);
}

