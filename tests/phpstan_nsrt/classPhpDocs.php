<?php

namespace ClassPhpDocsNamespace;

use function PHPStan\Testing\assertType;

/**
 * @method string string()
 * @method array arrayOfStrings()
 * @psalm-method array<string> arrayOfStrings()
 * @phpstan-method array<string, int> arrayOfInts()
 * @phan-method array<string> arrayOfStrings()
 * @method array arrayOfInts()
 * @method mixed overrodeMethod()
 * @method static mixed overrodeStaticMethod()
 */
class Foo
{
    public function __call($name, $arguments){}

    public static function __callStatic($name, $arguments){}

    public function doFoo()
    {
        assertType('string', $this->string());
        assertType('array<string>', $this->arrayOfStrings());
        assertType('array<string, int>', $this->arrayOfInts());
        assertType('mixed', $this->overrodeMethod());
        assertType('mixed', static::overrodeStaticMethod());
    }
}

/**
 * @phpstan-method string overrodeMethod()
 * @phpstan-method static int overrodeStaticMethod()
 */
class Child extends Foo
{
    public function doFoo()
    {
        assertType('string', $this->overrodeMethod());
        assertType('int', static::overrodeStaticMethod());
    }
}

