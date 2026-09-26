<?php

namespace Bug4351;

use function PHPStan\Testing\assertType;

class Thing
{
	public function doSomething(): void
	{
	}
}

class ParentC
{
	/** @var Thing|null */
	protected $thing;

	protected function __construct()
	{
		$this->thing = null;
	}
}

class HelloWorld extends ParentC
{
	public function __construct(Thing $thing)
	{
		assertType('Bug4351\Thing|null', $this->thing);
		$this->thing = $thing;
		assertType('Bug4351\Thing', $this->thing);

		parent::__construct();
		assertType('Bug4351\Thing|null', $this->thing);
	}

	public function doFoo(Thing $thing)
	{
		assertType('Bug4351\Thing|null', $this->thing);
		$this->thing = $thing;
		assertType('Bug4351\Thing', $this->thing);

		UnrelatedClass::doFoo();
		assertType('Bug4351\Thing', $this->thing);
	}

	public function doBar(Thing $thing)
	{
		assertType('Bug4351\Thing|null', $this->thing);
		$this->thing = $thing;
		assertType('Bug4351\Thing', $this->thing);

		UnrelatedClass::doStaticFoo();
		assertType('Bug4351\Thing', $this->thing);
	}
}

class UnrelatedClass
{

	public function doFoo(): void
	{

	}

	public static function doStaticFoo(): void
	{

	}

}
