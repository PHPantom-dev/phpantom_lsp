<?php

namespace Bug8361;

use function PHPStan\Testing\assertType;
use DateTimeInterface;

class HelloWorld
{
	public function sayHello(?DateTimeInterface $from = null, ?DateTimeInterface $to = null): void
	{
		if ($from || $to) {
			$operator = $from ? 'notBefore' : 'notAfter';
			assertType('DateTimeInterface', $from ?? $to);
			$date = ($from ?? $to)->format(DateTimeInterface::ATOM);
			assertType('DateTimeInterface', $from ?? $to);
		}
	}

	public function sayHello2(?DateTimeInterface $from = null, ?DateTimeInterface $to = null): void
	{
		if ($from || $to) {
			$operator = $from ? 'notBefore' : 'notAfter';
			$date = ($from ?? $to)->format(DateTimeInterface::ATOM);
			$date = ($from ?? $to)->format(DateTimeInterface::ATOM);
			assertType('DateTimeInterface', $from ?? $to);
		}
	}
}
