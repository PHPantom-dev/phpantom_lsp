<?php declare(strict_types = 1);

namespace Bug13342;

use stdClass;
use function PHPStan\Testing\assertType;

class Mixin { public bool $test = true; }

/** @mixin Mixin */
#[\AllowDynamicProperties]
class TestNormal {}

/** @mixin Mixin */
#[\AllowDynamicProperties]
class TestUniversalObjectCrate extends stdClass {}

function test(TestNormal $normal, TestUniversalObjectCrate $crate): void
{
	assertType('bool', $normal->test);
	assertType('bool', $crate->test);
}
