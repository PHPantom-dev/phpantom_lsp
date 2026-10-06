<?php

declare(strict_types=1);

namespace Bug12891;

use function PHPStan\Testing\assertType;

class Handler
{
	/** @var iterable<array-key, string> */
	private iterable $builders;

	/**
	 * @param iterable<array-key, string> $builders
	 */
	public function __construct(iterable $builders) {
		$this->builders = $builders;
		assertType('iterable<(int|string), string>', $builders);
		assertType('iterable<(int|string), string>', $this->builders);
	}
}
