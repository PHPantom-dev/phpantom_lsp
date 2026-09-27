<?php // lint >= 8.0

namespace Bug9105;

use function PHPStan\Testing\assertType;

class H
{
	public int|null $a = null;
	public self|null $b = null;

	public function h(): void
	{
		assertType('Bug9105\H|null', $this->b);
		if ($this->b?->a < 5) {
			echo '<5', PHP_EOL;
		}
		assertType('Bug9105\H|null', $this->b);
		if ($this->b?->a > 0) {
			echo '>0', PHP_EOL;
		}
		assertType('Bug9105\H|null', $this->b);
	}
}
