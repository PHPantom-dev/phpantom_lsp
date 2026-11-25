<?php

namespace Bug4708Two;

use function PHPStan\Testing\assertType;

/**
 * @param string[] $columns
 *
 * @return string[]|false
 */

function GenerateData($columns)
{
	$res = [];

	if (!empty($columns))
	{
		foreach ($columns as $col)
		{
			$res[$col] = $col;
		}
	}
	else
	{
		$res = false;
	}

	return $res;
}

/**
 * Get a bunch of configurational fields about an ASC.
 *
 * @return array
 */

function GetASCConfig()
{
	$columns = ['bsw', 'bew', 'utc', 'ssi'];

	$result = GenerateData($columns);

	if ($result === FALSE)
	{
		$result = ['result'  => FALSE,
			'dberror' => 'xyz'];
	}
	else
	{
		assertType('array<string>', $result);
		if (!isset($result['bsw']))
		{
			assertType('array<string>', $result);
			$result['bsw'] = 1;
		}
		else
		{
			$result['bsw'] = (int) $result['bsw'];
		}


		if (!isset($result['bew']))
		{
			$result['bew'] = 5;
		}
		else
		{
			$result['bew'] = (int) $result['bew'];
		}


		foreach (['utc', 'ssi'] as $field)
		{
			if (array_key_exists($field, $result))
			{
				$result[$field] = (int) $result[$field];
			}
		}

	}

	assertType('array{result: false, dberror: \'xyz\'}|non-empty-array<array-key, string|int>', $result); // PHPStan says `non-empty-array<int|string|false>`; PHPantom is more precise here

	return $result;
}
