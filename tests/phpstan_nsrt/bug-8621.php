<?php

namespace Bug8621;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<string> $data
	 */
	public function rows (array $data): void
	{
		$even = true;

		echo "<table>";
		foreach ($data as $datum)
		{
			$even = !$even;
			assertType('bool', $even);

			echo "<tr class='" . ($even ? 'even' :'odd') . "'>";
			echo "<td>{$datum}</td></tr>";
		}
		echo "</table>";
	}
}
