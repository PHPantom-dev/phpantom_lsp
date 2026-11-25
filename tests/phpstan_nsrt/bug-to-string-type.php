<?php

namespace BugToStringType;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class ParentClassWithToStringMixedReturn
{
    public function __toString()
    {
        return 'a';
    }
}

class WithParentMixedReturn extends ParentClassWithToStringMixedReturn
{
    public function __toString()
    {
        return 'value';
    }
}

class Consumer extends WithParentMixedReturn
{
    public function __toString()
    {
        return 'value';
    }
}

function test(Consumer $test): void
{
	assertType('string', $test->__toString());
	assertNativeType('mixed', $test->__toString());
}
