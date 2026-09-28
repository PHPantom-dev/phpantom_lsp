<?php

namespace Bug6404;


use function PHPStan\Testing\assertType;

class Foo
{
	public static function getCode(): int
	{
		return 1;
	}
}

class Bar
{
	private const FOOS = [
		Foo::class,
	];

	/**
	 * @var array<int, bool>
	 */
	private $someMap = [];

	public function build(): void
	{
		foreach (self::FOOS as $fooClass) {
			if (is_a($fooClass, Foo::class, true)) {
				assertType("'Bug6404\\\\Foo'", $fooClass);
				assertType('int', $fooClass::getCode());
				$this->someMap[$fooClass::getCode()] = true;
			}
		}
	}

	/**
	 * @param object[] $objects
	 * @return void
	 */
	public function build2(array $objects): void
	{
		foreach ($objects as $fooClass) {
			if (is_a($fooClass, Foo::class)) {
				assertType(Foo::class, $fooClass);
				assertType('int', $fooClass::getCode());
				$this->someMap[$fooClass::getCode()] = true;
			}
		}
	}

	/**
	 * @param mixed[] $mixeds
	 * @return void
	 */
	public function build3(array $mixeds): void
	{
		foreach ($mixeds as $fooClass) {
			if (is_a($fooClass, Foo::class, true)) {
				assertType('Bug6404\\Foo|class-string<Bug6404\\Foo>', $fooClass);
				assertType('int', $fooClass::getCode());
				$this->someMap[$fooClass::getCode()] = true;
			}
		}
	}

	/**
	 * @param class-string<Foo> $classString
	 * @return void
	 */
	public function doBar(string $classString): void
	{
		assertType("class-string<" . Foo::class . ">", $classString);
		assertType('int', $classString::getCode());
	}

	/**
	 * @return array<int, bool>
	 */
	public function getAll(): array
	{
		return $this->someMap;
	}
}
