<?php

namespace ComposerTreatPhpDocTypesAsCertainBug;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * Create a local dummy repository to run tests against!
	 *
	 * @param array<string, string|null> $files
	 */
	function setupDummyRepo(array $files): void
	{
		assertType('array<string, string|null>', $files);
		assertNativeType('array', $files);
		foreach ($files as $path => $content) {
			assertNativeType('non-empty-array', $files);
			assertType('string', $path);
			assertNativeType('(int|string)', $path);
			assertType('string|null', $content);
			assertNativeType('mixed', $content);
			assertType('string|null', $files[$path]);
			assertNativeType('mixed', $files[$path]);
			if ($files[$path] === null) {
				assertType('null', $files[$path]);
				assertNativeType('null', $files[$path]);
				$files[$path] = 'content';
				assertType('\'content\'', $files[$path]);
				assertNativeType('\'content\'', $files[$path]);
			}

			assertType('string', $files[$path]);
			assertNativeType('mixed~null', $files[$path]);
		}
	}

}
