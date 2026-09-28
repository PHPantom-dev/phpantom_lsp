<?php

namespace Bug12355;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-type AnimalData array{
 *  animalSpecificData: ValidType,
 *  habitat?: string,
 * }
 * @template ValidType of array{
 *  name: string,
 *  ...,
 * }
 */
abstract class Animal
{
	/**
	 * @param AnimalData $arg
	 */
	public function __construct(array $arg) {
		assertType('ValidType of array{name: string} (class Bug12355\Animal, argument)', $arg['animalSpecificData']);
		if (isset($arg['habitat'])) {
			//do things
		}
		assertType('ValidType of array{name: string} (class Bug12355\Animal, argument)', $arg['animalSpecificData']);
	}
}

/**
 * @template T of array{name: string, ...}
 * @param array{first: T, second: T, flag?: bool} $arg
 */
function testMergeWithDifferentObjects(array $arg): void
{
	assertType('T of array{name: string} (function Bug12355\testMergeWithDifferentObjects(), argument)', $arg['first']);

	// Modifying $arg in one branch causes different ConstantArrayType objects
	if (isset($arg['flag'])) {
		$arg['extra'] = true;
	}

	// After scope merge, $arg's value types for 'first' and 'second' go through
	// ConstantArrayType::mergeWith() which uses new self() — stripping TemplateConstantArrayType
	assertType('T of array{name: string} (function Bug12355\testMergeWithDifferentObjects(), argument)', $arg['first']);
	assertType('T of array{name: string} (function Bug12355\testMergeWithDifferentObjects(), argument)', $arg['second']);
}
