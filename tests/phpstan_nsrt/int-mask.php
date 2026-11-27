<?php declare(strict_types = 1);

namespace IntMask;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	const FOO_BAR = 1;
	const FOO_BAZ = 2;

	const BAZ_FOO = 1;
	const BAZ_BAR = 4;

	const BAR_INT = 1;
	const BAR_STR = '';

	/**
	 * @param int-mask-of<self::FOO_*> $one
	 * @param int-mask<self::FOO_BAR, self::FOO_BAZ> $two
	 * @param int-mask<1, 2, 8> $three
	 * @param int-mask<1, 4, 16, 64, 256, 1024> $four
	 * @param int-mask-of<self::BAZ_*> $five
	 */
	public static function test(int $one, int $two, int $three, int $four, int $five): void
	{
		assertType('int<0, 3>', $one);
		assertType('int<0, 3>', $two);
		assertType('0|1|2|3|8|9|10|11', $three);
		assertType('0|1|4|5|16|17|20|21|64|65|68|69|80|81|84|85|256|257|260|261|272|273|276|277|320|321|324|325|336|337|340|341|1024|1025|1028|1029|1040|1041|1044|1045|1088|1089|1092|1093|1104|1105|1108|1109|1280|1281|1284|1285|1296|1297|1300|1301|1344|1345|1348|1349|1360|1361|1364|1365', $four);
		assertType('0|1|4|5', $five);
	}

	/**
	 * @param int-mask-of<self::BAR_*> $one
	 * @param int-mask<0, 1, false> $two
	 */
	public static function invalid(int $one, int $two, int $three): void
	{
		assertType('int', $one); // not all constant integers
		assertType('int', $two); // not all constant integers
	}
}
