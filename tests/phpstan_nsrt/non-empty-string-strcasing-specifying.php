<?php

namespace NonEmptyStringStrCasing;

use function PHPStan\Testing\assertType;

class Foo {
	public function strtolower(string $s): void
	{
		if (strtolower($s) === 'hallo') {
		}
		assertType('string', $s);
		if ('hallo' === strtolower($s)) {
		}
		assertType('string', $s);

		$x = (strtolower($s) === 'hallo');
		assertType('string', $s);
		var_dump($x);

		$x = (strtolower($s) !== 'hallo');
		assertType('string', $s);
		var_dump($x);
	}

	public function strtoupper(string $s): void
	{
		if (strtoupper($s) === 'HA') {
		}
		assertType('string', $s);
		if ('hallo' === strtoupper($s)) {
		}
		assertType('string', $s);

		$x = (strtoupper($s) === 'HA');
		assertType('string', $s);
		var_dump($x);

		$x = (strtoupper($s) !== 'HA');
		assertType('string', $s);
		var_dump($x);
	}

	public function mb_strtoupper(string $s): void
	{
		if (mb_strtoupper($s) === 'HA') {
		}
		assertType('string', $s);
		if ('hallo' === mb_strtoupper($s)) {
		}
		assertType('string', $s);

		$x = (mb_strtoupper($s) === 'HA');
		assertType('string', $s);
		var_dump($x);

		$x = (mb_strtoupper($s) !== 'HA');
		assertType('string', $s);
		var_dump($x);
	}

	public function mb_strtolower(string $s): void
	{
		if (mb_strtolower($s) === 'hallo') {
		}
		assertType('string', $s);
		if ('hallo' === mb_strtolower($s)) {
		}
		assertType('string', $s);

		$x = (mb_strtolower($s) === 'hallo');
		assertType('string', $s);
		var_dump($x);

		$x = (mb_strtolower($s) !== 'hallo');
		assertType('string', $s);
		var_dump($x);
	}

	public function ucfirst(string $s): void
	{
		if (ucfirst($s) === 'Hallo') {
		}
		assertType('string', $s);
		if ('hallo' === ucfirst($s)) {
		}
		assertType('string', $s);

		$x = (ucfirst($s) === 'Hallo');
		assertType('string', $s);
		var_dump($x);

		$x = (ucfirst($s) !== 'Hallo');
		assertType('string', $s);
		var_dump($x);
	}

	public function lcfirst(string $s): void
	{
		if (lcfirst($s) === 'hallo') {
		}
		assertType('string', $s);
		if ('hallo' === lcfirst($s)) {
		}
		assertType('string', $s);

		$x = (lcfirst($s) === 'hallo');
		assertType('string', $s);
		var_dump($x);

		$x = (lcfirst($s) !== 'hallo');
		assertType('string', $s);
		var_dump($x);
	}

	public function ucwords(string $s): void
	{
		if (ucwords($s) === 'Hallo') {
		}
		assertType('string', $s);
		if ('hallo' === ucwords($s)) {
		}
		assertType('string', $s);

		$x = (ucwords($s) === 'Hallo');
		assertType('string', $s);
		var_dump($x);

		$x = (ucwords($s) !== 'Hallo');
		assertType('string', $s);
		var_dump($x);
	}

}
