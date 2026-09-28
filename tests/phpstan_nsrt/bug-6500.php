<?php

namespace Bug6500;

use function PHPStan\Testing\assertType;

class DatePeriodExample extends \DatePeriod {

	public static function test(): void {
		$self = new self(new \DateTimeImmutable('2022-01-01'), new \DateInterval('P1M'), new \DateTimeImmutable('2022-01-31'));
		assertType(self::class, $self);
	}

}
