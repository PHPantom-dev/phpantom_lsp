<?php

namespace Bug7550;

use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @return static
	 */
	private function _load()
	{
		return $this;
	}

	public function reload(): void
	{
		$res = $this->_load();
		assertType('static(Bug7550\Foo)', $res);
		if ($res !== $this) {
			throw new \Exception('y');
		}

	}
}
