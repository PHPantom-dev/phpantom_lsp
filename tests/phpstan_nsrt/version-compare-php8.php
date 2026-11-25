<?php // lint >= 8.0

declare(strict_types=1);

namespace VersionComparePHP8;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param string  $string
	 * @param '<'|'>' $unionValid
	 * @param '<'|'a' $unionBoth
	 * @param 'a'|'b' $unionInvalid
	 */
	public function fgetss(
		string $string,
		string $unionValid,
		string $unionBoth,
		string $unionInvalid,
	) : void
	{
		assertType('bool', \version_compare($string, $string, $string));

		assertType('bool', \version_compare('Foo','Bar', $string));
	}
}
