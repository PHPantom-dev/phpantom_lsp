<?php

namespace Bug4231;

use function PHPStan\Testing\assertType;

function (): void {
	$property = new \ReflectionProperty($this, 'data');

	if (is_null($property->getValue($this))) {
		assertType('null', $property->getValue($this));
		$property->setValue($this, 'Some value which is not null');
		assertType('mixed', $property->getValue($this));
	}
};

function (): void {
	$property = new \ReflectionProperty($this, 'data');

	if (is_null($property->getValue($this))) {
		assertType('null', $property->getValue($this));
		$property->setValue($this, 'Some value which is not null');

		$value = $property->getValue($this);
		assertType('mixed', $value);
	}
};
