<?php

namespace Bug5501;

use function PHPStan\Testing\assertType;

class Durable
{

	/** @var int */
	private $damage = 0;

	/** @var int */
	private $prop2 = 0;

	/** @var mixed[] */
	private $array;

	public function applyDamage(int $amount): void
	{
		$this->prop2 = 5;

		if ($this->isBroken()){
			return;
		}

		assertType('false', $this->isBroken());
		assertType('5', $this->prop2);

		$this->damage = min($this->damage + $amount, 5);

		assertType('bool', $this->isBroken());
		assertType('5', $this->prop2);
	}

	public function applyDamage2(int $amount): void
	{
		$this->prop2 = 5;

		if ($this->isBroken()){
			return;
		}

		assertType('false', $this->isBroken());
		assertType('5', $this->prop2);

		$this->array['foo'] = min($this->damage + $amount, 5);

		assertType('bool', $this->isBroken());
		assertType('5', $this->prop2);
	}

	protected function onBroken(): void
	{

	}

	public function isBroken(): bool{
		return $this->damage >= 5;
	}
}
