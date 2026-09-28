<?php

namespace Bug6635;

use function PHPStan\Testing\assertType;

interface A {
	public function getValue(): int;
}

class HelloWorld
{
	/**
	 * @template T
	 *
	 * @param T $block
	 *
	 * @return T
	 */
	protected function sayHelloBug(mixed $block): mixed {
		assertType('T (method Bug6635\HelloWorld::sayHelloBug(), argument)', $block);
		if ($block instanceof A) {
			assertType('Bug6635\A&T (method Bug6635\HelloWorld::sayHelloBug(), argument)', $block);
			echo 1;
		} else {
			assertType('T of mixed~Bug6635\A (method Bug6635\HelloWorld::sayHelloBug(), argument)', $block);
		}

		assertType('T (method Bug6635\HelloWorld::sayHelloBug(), argument)', $block);

		return $block;
	}
}
