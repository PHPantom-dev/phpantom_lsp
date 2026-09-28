<?php declare(strict_types = 1);

namespace Bug12125;

use ArrayAccess;
use Exception;
use stdClass;

use function PHPStan\Testing\assertType;

/** @var ArrayAccess<array-key, stdClass>|array<array-key, stdClass> $bug */
$bug = [];

assertType('stdClass|null', $bug['key'] ?? null);

interface MyInterface {
	/** @return array<string, string> | ArrayAccess<string, string> */
	public function getStrings(): array | ArrayAccess;
}

function myFunction(MyInterface $container): string {
	$strings = $container->getStrings();
	assertType('array<string, string>|ArrayAccess<string, string>', $strings);
	return $strings['test'];
}

function myOtherFunction(MyInterface $container): string {
	$strings = $container->getStrings();
	assertType('array<string, string>|ArrayAccess<string, string>', $strings);
	if (isset($strings['test'])) {
		assertType('string', $strings['test']);
		return $strings['test'];
	} else {
		throw new Exception();
	}
}
