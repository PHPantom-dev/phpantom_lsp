<?php

namespace NonEmptyStringFileFamily;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param non-empty-string $nonES
	 * @param non-falsy-string $nonFalsyString
	 * @param numeric-string $numericString
	 */
	public function strTypes($nonES, $nonFalsyString, $numericString) {
		if (dir($nonES)) {
			assertType('non-empty-string', $nonES);
		} else {
			assertType('non-empty-string', $nonES);
		}
		assertType('non-empty-string', $nonES);

		if (dir($nonFalsyString)) {
			assertType('non-falsy-string', $nonFalsyString);
		}
		assertType('non-falsy-string', $nonFalsyString);

		if (dir($numericString)) {
		}
		assertType('numeric-string', $numericString);
	}

	public function fileNonEmptyStrings(string $s): void
	{
		if (dir($s)) {
		} else {
			assertType('string', $s);
		}
		assertType('string', $s);

		if (chdir($s)) {
		}
		assertType('string', $s);

		if (chroot($s)) {
		}
		assertType('string', $s);

		if (opendir($s)) {
		}
		assertType('string', $s);

		if (scandir($s)) {
		}
		assertType('string', $s);

		if (is_writable($s)) {
		}
		assertType('string', $s);

		if (is_readable($s)) {
		}
		assertType('string', $s);

		if (is_executable($s)) {
		}
		assertType('string', $s);

		if (file($s)) {
		}
		assertType('string', $s);

		if (file_get_contents($s)) {
		}
		assertType('string', $s);

		if (file_put_contents($s, '')) {
		}
		assertType('string', $s);

		if (fileatime($s)) {
		}
		assertType('string', $s);

		if (filectime($s)) {
		}
		assertType('string', $s);

		if (filegroup($s)) {
		}
		assertType('string', $s);

		if (fileinode($s)) {
		}
		assertType('string', $s);

		if (filemtime($s)) {
		}
		assertType('string', $s);

		if (fileowner($s)) {
		}
		assertType('string', $s);

		if (fileperms($s)) {
		}
		assertType('string', $s);

		if (filesize($s)) {
		}
		assertType('string', $s);

		if (filetype($s)) {
		}
		assertType('string', $s);

		if (fopen($s, "r")) {
		}
		assertType('string', $s);

		if (linkinfo($s)) {
		}
		assertType('string', $s);

		if (lstat($s)) {
		}
		assertType('string', $s);

		if (mkdir($s)) {
		}
		assertType('string', $s);

		if (readfile($s)) {
		}
		assertType('string', $s);

		if (readlink($s)) {
		}
		assertType('string', $s);

		if (realpath($s)) {
		}
		assertType('string', $s);

		if (rmdir($s)) {
		}
		assertType('string', $s);

		if (stat($s)) {
		}
		assertType('string', $s);

		if (touch($s)) {
		}
		assertType('string', $s);

		if (unlink($s)) {
		}
		assertType('string', $s);

	}

}
