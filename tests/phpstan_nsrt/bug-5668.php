<?php

namespace Bug5668;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<int, 'test'|'bar'> $in
	 */
	function has(array $in): void
	{
		assertType('bool', in_array('test', $in, true));
	}

	/**
	 * @param array<int, 'test'> $in
	 */
	function has2(array $in): void
	{
		assertType('bool', in_array('test', $in, true));
	}

	/**
	 * @param non-empty-array<int, 'test'|'bar'> $in
	 */
	function has3(array $in, string $s): void
	{
		assertType('bool', in_array('test', $in, true));
		assertType('bool', in_array(rand() ? 'test' : 'bar', $in, true));
		assertType('bool', in_array($s, $in, true));
	}


	/**
	 * @param non-empty-array<int, 'test'> $in
	 */
	function has4(array $in): void
	{
	}

    /**
     * @param non-empty-array<int, string> $in
     */
    function has5(array $in): void
    {
        assertType('bool', in_array('test', $in, true));
    }

}
