<?php declare(strict_types=1);

namespace Bug10002;

use function PHPStan\Testing\assertType;

class Foo
{
	public function bar(int $month): bool
	{
		$day1 = new \DateTime('2022-01-01');
		$day2 = new \DateTime('2022-05-01');

		$monthDay1 = (int) $day1->format('n');
		$monthDay2 = (int) $day2->format('n');


		if($monthDay1 === $month){
			return true;
		}

		assertType('bool', $monthDay2 === $month);

		return $monthDay2 === $month;
	}

	public function bar1(int $month): bool
	{
		$day1 = new \DateTime('2022-01-01');
		$day2 = new \DateTime('2022-05-01');

		$monthDay1 = (int) $day1->format('n');
		$monthDay2 = (int) $day2->format('n');


		if($month === $monthDay1){
			return true;
		}

		assertType('bool', $month === $monthDay2);

		return $monthDay2 === $month;
	}
}
