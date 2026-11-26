<?php declare(strict_types = 1);

namespace Bug7374;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/** @return class-string<self>&literal-string */
	public static function getClass(): string {
		return self::class;
	}

	public function build(): void {
		$class = self::getClass();
		assertType(self::class, new $class());
	}
}
