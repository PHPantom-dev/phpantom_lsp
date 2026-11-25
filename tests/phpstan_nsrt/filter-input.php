<?php declare(strict_types=1);

namespace FilterInput;

use function PHPStan\Testing\assertType;

class FilterInput
{

	public function invalidTypesOrVarNames($mixed): void
	{
		assertType('int|false|null', filter_input(INPUT_GET, $mixed, FILTER_VALIDATE_INT));
	}

	public function supportedSuperGlobals(): void
	{
		assertType('int|false|null', filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT));
		assertType('int|false|null', filter_input(INPUT_POST, 'foo', FILTER_VALIDATE_INT));
		assertType('int|false|null', filter_input(INPUT_COOKIE, 'foo', FILTER_VALIDATE_INT));
		assertType('int|false|null', filter_input(INPUT_SERVER, 'foo', FILTER_VALIDATE_INT));
		assertType('int|false|null', filter_input(INPUT_ENV, 'foo', FILTER_VALIDATE_INT));
	}

	public function inputTypeUnion(): void
	{
		assertType('int|false|null', filter_input(rand(0, 1) ? INPUT_GET : INPUT_POST, 'foo', FILTER_VALIDATE_INT));
	}

	public function doFoo(string $foo): void
	{
		assertType('int|false|null', filter_input(INPUT_GET, $foo, FILTER_VALIDATE_INT));
		assertType('int|false|null', filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT));
		assertType('int|false|null', filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT, ['flags' => FILTER_NULL_ON_FAILURE]));
		assertType("'invalid'|int|null", filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT, ['options' => ['default' => 'invalid']]));
		assertType('array<int|false>|null', filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT, ['flags' => FILTER_FORCE_ARRAY])); // SKIP: filter_input() and filter_var() with FILTER_FORCE_ARRAY return mixed
		assertType('array<int|null>|false', filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT, ['flags' => FILTER_FORCE_ARRAY|FILTER_NULL_ON_FAILURE])); // SKIP: filter_input() and filter_var() with FILTER_FORCE_ARRAY return mixed
		assertType('0|int<17, 19>|null', filter_input(INPUT_GET, 'foo', FILTER_VALIDATE_INT, ['options' => ['default' => 0, 'min_range' => 17, 'max_range' => 19]]));
	}

}
