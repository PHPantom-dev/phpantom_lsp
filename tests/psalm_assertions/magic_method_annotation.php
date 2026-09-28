<?php
// Source: Psalm MagicMethodAnnotationTest.php
// Auto-extracted by scripts/extract_psalm_tests.php
// Do not edit manually — re-run the extraction script instead.

// Test: validSimpleAnnotations
namespace PsalmTest_magic_method_annotation_1 {
    class ParentClass {
        public function __call(string $name, array $args) {}
    }

    /**
     * @method string getString() dsa sada
     * @method  void setInteger(int $integer) dsa sada
     * @method setString(int $integer) dsa sada
     * @method setMixed(mixed $foo) dsa sada
     * @method setImplicitMixed($foo) dsa sada
     * @method setAnotherImplicitMixed( $foo, $bar,$baz) dsa sada
     * @method setYetAnotherImplicitMixed( $foo  ,$bar,  $baz    ) dsa sada
     * @method  getBool(string $foo)  :   bool dsa sada
     * @method (string|int)[] getArray() with some text dsa sada
     * @method (callable() : string) getCallable() dsa sada
     */
    class Child extends ParentClass {}

    $child = new Child();

    $a = $child->getString();
    $child->setInteger(4);
    /** @psalm-suppress MixedAssignment */
    $b = $child->setString(5);
    $c = $child->getBool("hello");
    $d = $child->getArray();
    $e = $child->getCallable();
    $child->setMixed("hello");
    $child->setMixed(4);
    $child->setImplicitMixed("hello");
    $child->setImplicitMixed(4);

    assertType('string', $a);
    assertType('mixed', $b);
    assertType('bool', $c);
    assertType('array<string|int>', $d);
    assertType('callable(): string', $e);
}

// Test: validSimpleAnnotationsWithStatic
namespace PsalmTest_magic_method_annotation_2 {
    class ParentClass {
        public function __callStatic(string $name, array $args) {}
    }

    /**
     * @method static string getString() dsa sada
     * @method static void setInteger(int $integer) dsa sada
     * @method static mixed setString(int $integer) dsa sada
     * @method static mixed setMixed(mixed $foo) dsa sada
     * @method static mixed setImplicitMixed($foo) dsa sada
     * @method static mixed setAnotherImplicitMixed( $foo, $bar,$baz) dsa sada
     * @method static mixed setYetAnotherImplicitMixed( $foo  ,$bar,  $baz    ) dsa sada
     * @method static bool getBool(string $foo)   dsa sada
     * @method static (string|int)[] getArray() with some text dsa sada
     * @method static (callable() : string) getCallable() dsa sada
     * @method static static getInstance() dsa sada
     */
    class Child extends ParentClass {}

    $a = Child::getString();
    Child::setInteger(4);
    /** @psalm-suppress MixedAssignment */
    $b = Child::setString(5);
    $c = Child::getBool("hello");
    $d = Child::getArray();
    $e = Child::getCallable();
    $f = Child::getInstance();
    Child::setMixed("hello");
    Child::setMixed(4);
    Child::setImplicitMixed("hello");
    Child::setImplicitMixed(4);

    assertType('string', $a);
    assertType('mixed', $b);
    assertType('bool', $c);
    assertType('array<string|int>', $d);
    assertType('callable(): string', $e);
    assertType('PsalmTest_magic_method_annotation_2\Child', $f);
}

// Test: validStaticAnnotationWithDefault
namespace PsalmTest_magic_method_annotation_3 {
    class ParentClass {
        public static function __callStatic(string $name, array $args) {}
    }

    /**
     * @method static string getString(int $foo) with some more text
     */
    class Child extends ParentClass {}

    $child = new Child();

    $a = $child::getString(5);

    assertType('string', $a);
}

// Test: validUnionAnnotations
namespace PsalmTest_magic_method_annotation_4 {
    class ParentClass {
        public function __call(string $name, array $args) {}
    }

    /**
     * @method setBool(string $foo, string|bool $bar)  :   bool dsa sada
     * @method void setAnotherArray(int[]|string[] $arr = [], int $foo = 5) with some more text
     */
    class Child extends ParentClass {}

    $child = new Child();

    $b = $child->setBool("hello", true);
    $c = $child->setBool("hello", "true");
    $child->setAnotherArray(["boo"]);

    assertType('bool', $b); 
    assertType('bool', $c); 
}

// Test: magicMethodReturnSelf
namespace PsalmTest_magic_method_annotation_5 {
    /**
     * @method static self getSelf()
     * @method $this getThis()
     */
    class C {
        public static function __callStatic(string $c, array $args) {}
        public function __call(string $c, array $args) {}
    }

    $a = C::getSelf();
    $b = (new C)->getThis();

    assertType('C', $a);
    assertType('PsalmTest_magic_method_annotation_5\C', $b);
}

// Test: allowMagicMethodStatic
namespace PsalmTest_magic_method_annotation_6 {
    /** @method static getStatic() */
    class C {
        public function __call(string $c, array $args) {}
    }

    class D extends C {}

    $c = (new C)->getStatic();
    $d = (new D)->getStatic();

    assertType('PsalmTest_magic_method_annotation_6\C', $c); 
    assertType('PsalmTest_magic_method_annotation_6\D', $d);
}

// Test: validSimplePsalmAnnotations
namespace PsalmTest_magic_method_annotation_7 {
    class ParentClass {
        public function __call(string $name, array $args) {}
    }

    /**
     * @psalm-method string getString() dsa sada
     * @psalm-method  void setInteger(int $integer) dsa sada
     */
    class Child extends ParentClass {}

    $child = new Child();

    $a = $child->getString();
    $child->setInteger(4);

    assertType('string', $a);
}

// Test: overrideMethodAnnotations
namespace PsalmTest_magic_method_annotation_8 {
    class ParentClass {
        public function __call(string $name, array $args) {}
    }

    /**
     * @method int getString() dsa sada
     * @method  void setInteger(string $integer) dsa sada
     * @psalm-method string getString() dsa sada
     * @psalm-method  void setInteger(int $integer) dsa sada
     */
    class Child extends ParentClass {}

    $child = new Child();

    $a = $child->getString();
    $child->setInteger(4);

    assertType('string', $a);
}

// Test: returnThisShouldKeepGenerics
namespace PsalmTest_magic_method_annotation_9 {
    /**
     * @template E
     * @method $this foo()
     */
    class A
    {
        public function __call(string $name, array $args) {}
    }

    /**
     * @template E
     * @method $this foo()
     */
    interface I {}

    class B {}

    /** @var A<B> $a */
    $a = new A();
    $b = $a->foo();

    /** @var I<B> $i */
    $c = $i->foo();

    assertType('A<B>', $b);
    assertType('I<B>', $c);
}

// Test: genericsOfInheritedMethodsShouldBeResolved
namespace PsalmTest_magic_method_annotation_10 {
    /**
     * @template E
     * @method E get()
     */
    interface I {}

    /**
     * @template E
     * @implements I<E>
     */
    class A implements I
    {
        public function __call(string $name, array $args) {}
    }

    /**
     * @template E
     * @extends I<E>
     */
    interface I2 extends I {}

    class B {}

    /**
     * @template E
     * @method E get()
     */
    class C
    {
        public function __call(string $name, array $args) {}
    }

    /**
     * @template E
     * @extends C<E>
     */
    class D extends C {}

    /** @var A<B> $a */
    $a = new A();
    $b = $a->get();

    /** @var I2<B> $i */
    $c = $i->get();

    /** @var D<B> $d */
    $d = new D();
    $e = $d->get();

    assertType('B', $b);
    assertType('B', $c);
    assertType('B', $e);
}

// Test: magicStaticMethodInheritanceWithoutCallStatic_WithReturnAndManyArgs
namespace PsalmTest_magic_method_annotation_11 {
    /**
     * @method static void bar()
     */
    class A {}
    class B extends A {}

    /** @psalm-suppress UndefinedMethod, MixedAssignment */
    $a = B::bar(123, "whatever");

    // PHPantom, like PHPStan, honours `@method` without `__callStatic()`, and a `void` method's call yields `null`.
    assertType('null', $a);
}

// Test: magicMethodInheritanceWithoutCall_WithReturnAndManyArgs
namespace PsalmTest_magic_method_annotation_12 {
    /**
     * @method void bar()
     */
    class A {}
    class B extends A {}

    $obj = new B();

    /** @psalm-suppress UndefinedMethod, MixedAssignment */
    $a = $obj->bar(123, "whatever");

    // PHPantom, like PHPStan, honours `@method` without `__call()`, and a `void` method's call yields `null`.
    assertType('null', $a);
}
