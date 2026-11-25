<?php

namespace DependentVariableTypeGuardSameAsType;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

class Foo
{

	/**
	 * @return \Generator|mixed[]|null
	 */
	public function getArrayOrNull(): ?iterable
	{
		return null;
	}

	public function doFoo(): void
	{
		$associationData = $this->getArrayOrNull();
		if ($associationData === null) {
		} else {
			$itemsCounter = 0;
			assertType('0', $itemsCounter);
			assertType('Generator|array', $associationData); // PHPStan reads `\\Generator|mixed[]` as an iterable `Generator`; PHPantom reads the union as written
			foreach ($associationData as $row) {
				$itemsCounter++;
			}

			assertType('Generator|array', $associationData); // PHPStan reads `\\Generator|mixed[]` as an iterable `Generator`; PHPantom reads the union as written

		}
	}

	public function doBar(float $f, bool $b): void
	{
		if ($f !== 1.0) {
			$foo = 'test';
		}

		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);

		if ($f !== 1.0) {
			assertType('float', $f);
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo); // could be Yes, but float type is not subtractable
		} else {
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo); // could be No, but float type is not subtractable
		}

		if ($f !== 2.0) {
			assertType('float', $f);
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		} else {
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		}

		if ($f !== 1.0) {
			assertType('float', $f);
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		} else {
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		}

		if ($b) {
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		} else {
			assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
		}

		assertVariableCertainty(TrinaryLogic::createMaybe(), $foo);
	}

}
