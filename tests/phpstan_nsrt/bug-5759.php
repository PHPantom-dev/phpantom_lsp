<?php

namespace Bug5759;

use function PHPStan\Testing\assertType;

interface ITF
{
	const FIELD_A = 1;
	const FIELD_B = 2;
	const FIELD_C = 3;
}

class Foo
{

	/** @param array<ITF::FIELD_*> $fields */
	function strict(array $fields): void
	{
		assertType('bool', in_array(ITF::FIELD_A, $fields, true));
	}


	/** @param array<ITF::FIELD_*> $fields */
	function loose(array $fields): void
	{
		assertType('bool', in_array(ITF::FIELD_A, $fields, false));
	}

	function another(): void
	{
		/** @var array<'source'|'dist'> $arr */
		$arr = ['source'];

		assertType('bool', in_array('dist', $arr, true));
		assertType('bool', in_array('dist', $arr));
	}

}
