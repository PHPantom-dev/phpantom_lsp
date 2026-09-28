<?php

namespace FilterVarDynamicReturnTypeExtensionRegression;

use PHPStan\Type\ConstantScalarType;
use PHPStan\Type\MixedType;
use PHPStan\Type\Type;
use PHPStan\Type\TypeCombinator;
use function PHPStan\Testing\assertType;

class Test {

	/**
	 * @return array{default: Type, range?: Type}
	 */
	public function getOtherTypes()
	{

	}

	public function determineExactType(): ?Type
	{

	}

	public function test()
	{

		$exactType = $this->determineExactType();
		$type = $exactType ?? new MixedType();
		$otherTypes = $this->getOtherTypes();

		assertType('array{default: PHPStan\Type\Type, range?: PHPStan\Type\Type}', $otherTypes);
		if (isset($otherTypes['range'])) {
			assertType('array{default: PHPStan\Type\Type, range: PHPStan\Type\Type}', $otherTypes);
			if ($type instanceof ConstantScalarType) {
				if ($otherTypes['range']->isSuperTypeOf($type)->no()) {
					$type = $otherTypes['default'];
				}
				assertType('array{default: PHPStan\Type\Type, range: PHPStan\Type\Type}', $otherTypes);
				unset($otherTypes['default']);
				assertType('array{range: PHPStan\Type\Type}', $otherTypes);
			} else {
				$type = $otherTypes['range'];
				assertType('array{default: PHPStan\Type\Type, range: PHPStan\Type\Type}', $otherTypes);
			}
			// Same shape; PHPantom lists the keys in the order the branches wrote them.
			assertType('array{range: Type, default?: Type}', $otherTypes);
		}
		if ($exactType !== null) {
			unset($otherTypes['default']);
			assertType('array{range?: PHPStan\Type\Type}', $otherTypes);
		}
		assertType('array{default?: PHPStan\Type\Type, range?: PHPStan\Type\Type}', $otherTypes);
		if (isset($otherTypes['default']) && $otherTypes['default']->isSuperTypeOf($type)->no()) {
			$type = TypeCombinator::union($type, $otherTypes['default']);
		}
	}
}
