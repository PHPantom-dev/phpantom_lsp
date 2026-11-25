<?php declare(strict_types=1);

namespace Superglobals;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Superglobals
{

	public function originalTypes(): void
	{
		assertType('array<mixed>', $GLOBALS);
		assertType('array<mixed>', $_SERVER);
		assertType('array<mixed>', $_GET);
		assertType('array<mixed>', $_POST);
		assertType('array<mixed>', $_FILES);
		assertType('array<mixed>', $_COOKIE);
		assertType('array<mixed>', $_SESSION);
		assertType('array<mixed>', $_REQUEST);
		assertType('array<mixed>', $_ENV);
	}

	public function canBeOverwritten(): void
	{
		$GLOBALS = [];
		assertType('array{}', $GLOBALS);
		assertNativeType('array{}', $GLOBALS);
	}

	public function canBePartlyOverwritten(): void
	{
		$GLOBALS['foo'] = 'foo';
		assertNativeType("non-empty-array<mixed>&hasOffsetValue('foo', 'foo')", $GLOBALS);
	}

	public function canBeNarrowed(): void
	{
		if (isset($GLOBALS['foo'])) {
			assertNativeType("non-empty-array<mixed>&hasOffset('foo')", $GLOBALS); // https://github.com/phpstan/phpstan/issues/8395
		} else {
			assertType('array<mixed>', $GLOBALS);
			assertNativeType('array<mixed>', $GLOBALS);
		}
		assertType('array<mixed>', $GLOBALS);
		assertNativeType('array<mixed>', $GLOBALS);
	}

}

function functionScope() {
	assertType('array<mixed>', $GLOBALS);
	assertNativeType('array<mixed>', $GLOBALS);
}

assertType('array<mixed>', $GLOBALS);
assertNativeType('array<mixed>', $GLOBALS);

function badNarrowing() {
	if (empty($_GET['id'])) {
		echo "b";
	} else {
		echo "b";
	}
	assertType('array<mixed>', $_GET);
	assertType('mixed', $_GET['id']);
};
