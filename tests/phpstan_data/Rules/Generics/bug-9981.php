<?php declare(strict_types = 1);

namespace Bug9981;

use function PHPStan\Testing\assertType;

interface File {
}

/**
 * @template TKey of array-key
 * @template T of File|Directory
 * @extends \Traversable<TKey,T>
 */
interface Directory extends \Traversable {
}

class Foo
{

	/**
	 * @param Directory<int, File> $d
	 * @return void
	 */
	public function doFoo(Directory $d): void
	{
		foreach ($d as $k => $v) {
			assertType('int', $k);
			assertType(File::class, $v);
		}
	}

	/**
	 * @param Directory<int, Directory> $d
	 * @return void
	 */
	public function doBar(Directory $d): void
	{
		foreach ($d as $k => $v) {
			assertType('int', $k);
			assertType(Directory::class, $v);
		}
	}

	/**
	 * @param Directory<int, File|Directory> $d
	 * @return void
	 */
	public function doBaz(Directory $d): void
	{
		foreach ($d as $k => $v) {
			assertType('int', $k);
			assertType('Bug9981\\Directory|Bug9981\\File', $v);
		}
	}

}
