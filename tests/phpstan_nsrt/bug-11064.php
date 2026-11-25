<?php declare(strict_types = 1);

namespace Bug11064;

use function PHPStan\Testing\assertType;

interface IClass {}
class ClassA implements IClass {}
class ClassB implements IClass {}

/**
 * @param null|'nil'|IClass $val
 */
function test($val): string {
	switch (true) {
		case $val === null:
		case $val === 'nil':
			assertType("'nil'|null", $val);
			return 'null';

		case $val instanceof ClassA:
			assertType('Bug11064\\ClassA', $val);
			return 'class a';

		default:
			throw new RuntimeException('unsupported class: ' . get_class($val));
	}
}

