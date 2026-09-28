<?php // lint >= 8.0

declare(strict_types=1);

namespace Bug9293;

use function PHPStan\Testing\assertType;

class B
{
	public function int(): int
	{
		return 0;
	}

	public function mixed(): mixed
	{
		return new self();
	}
}

/**
 * @var null|B $b
 */
$b = null;

assertType('Bug9293\B|null', $b);

$b?->mixed()->int() ?? 0;

assertType('Bug9293\B|null', $b);

$b?->int() ?? 0;
