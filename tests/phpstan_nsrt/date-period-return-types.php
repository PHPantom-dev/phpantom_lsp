<?php declare(strict_types=1);

namespace DatePeriodReturnTypes;

use DateInterval;
use DatePeriod;
use DateTime;
use DateTimeImmutable;
use function PHPStan\Testing\assertType;

$start = new DateTime('2012-07-01');
$interval = new DateInterval('P7D');
$end = new DateTime('2012-07-31');
$recurrences = 4;
$iso = 'R4/2012-07-01T00:00:00Z/P7D';

$datePeriodList = [];

$datePeriod = new DatePeriod($start, $interval, $end);
assertType(\DatePeriod::class . '<DateTime, DateTime, null>', $datePeriod);
assertType(\DateTime::class, $datePeriod->getEndDate());
assertType('null', $datePeriod->getRecurrences());
$datePeriodList[] = $datePeriod;

foreach ($datePeriod as $k => $v) {
	assertType('int', $k);
	assertType('DateTime', $v);
}

$datePeriod = new DatePeriod($start, $interval, $recurrences);
assertType(\DatePeriod::class . '<DateTime, null, 4>', $datePeriod);
assertType('null', $datePeriod->getEndDate());
assertType('4', $datePeriod->getRecurrences());
$datePeriodList[] = $datePeriod;

$datePeriod = new DatePeriod($iso);
assertType(\DatePeriod::class . '<DateTime, null, int>', $datePeriod);
assertType('null', $datePeriod->getEndDate());
assertType('int', $datePeriod->getRecurrences());
$datePeriodList[] = $datePeriod;

/** @var DatePeriod $datePeriod */
$datePeriod = $datePeriodList[random_int(0, 2)];
assertType(\DatePeriod::class, $datePeriod);
assertType(\DateTimeInterface::class . '|null', $datePeriod->getEndDate());
assertType('int|null', $datePeriod->getRecurrences());

class Foo
{
	private DatePeriod $period;

	public function doFoo(DateTimeImmutable $fromDate, DateTimeImmutable $toDate): void
	{
		$this->period = new DatePeriod($fromDate, new DateInterval('P1D'), $toDate->modify('+1 day'));
	}
}
