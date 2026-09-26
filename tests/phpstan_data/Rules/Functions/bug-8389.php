<?php

namespace Bug8389;

use function PHPStan\Testing\assertType;

/**
 * @param mixed $values
 *
 * @return bool
 *
 * @phpstan-assert-if-false null $values
 */
function check(...$values)
{
        foreach ($values as $value) {
            if ($value === null) {
                return false;
            }
        }
        return true;
}

/**
 * @param string $str1
 * @param string $str2
 *
 * @return void
 */
function doSomething($str1, $str2)
{
}

/**
 * @param string|null $input1
 * @param string|null $input2
 */
function foo($input1, $input2) {
	if (!check($input1, $input2)) {
		assertType('null', $input1);
		assertType('null', $input2);

		throw new \Exception();
	}

	assertType('string', $input1);
	assertType('string', $input2);

	doSomething($input1, $input2);
}
