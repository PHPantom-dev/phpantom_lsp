<?php

namespace SpecifiedFunctionCall;

use function PHPStan\Testing\assertType;

class IsFileChecks
{

	public function isFile(string $autoloadFile)
	{
		if (\is_file($autoloadFile) === true) {
			assertType('true', \is_file($autoloadFile));
			if (\is_file($autoloadFile) === true) {
				assertType('true', \is_file($autoloadFile));
			}
		}
	}

	public function isFileAnother(string $autoloadFile, string $other)
	{
		if (\is_file($autoloadFile) === true) {
			assertType('true', \is_file($autoloadFile));
			$autoloadFile = $other;
			assertType('bool', \is_file($autoloadFile));
			if (\is_file($autoloadFile) === true) {
				assertType('true', \is_file($autoloadFile));
			}
		}
	}

}
