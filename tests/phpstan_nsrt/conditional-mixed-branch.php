<?php

// A conditional return type whose selected branch is `mixed` resolves to
// `mixed` (an informative "value of unknown type"), not to no type at all,
// so downstream narrowing can still refine it.

use function PHPStan\Testing\assertType;

/** @return ($key is string ? mixed : null) */
function sessionValue(?string $key = null)
{
	return null;
}

class MixedBranchConditional
{
	public function selectedBranchIsMixed(): void
	{
		$file = sessionValue('file');
		assertType('mixed', $file);
	}

	public function mixedNarrowsThroughGuards(): void
	{
		$file = sessionValue('file');
		if (!is_string($file)) {
			$file = null;
		}
		if ($file) {
			assertType('string', $file);
		}
	}
}
