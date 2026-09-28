<?php declare(strict_types = 1);

namespace CountConstArray;

use function PHPStan\debugScope;
use function PHPStan\dumpType;
use function PHPStan\Testing\assertType;

final class Foo
{

	public function sayHello(): void
	{
		$expectedDaysResult = [
			'2019-01-04' => [
				'17:00',
				'evening',
			],
			'2019-01-05' => [
				'07:00',
				'morning',
			],
			'2019-01-06' => [
				'12:00',
				'afternoon',
			],
			'2019-01-07' => [
				'10:00',
				'11:00',
				'12:00',
				'13:00',
				'14:00',
				'15:00',
				'16:00',
				'17:00',
				'morning',
				'afternoon',
				'evening',
			],
			'2019-01-08' => [
				'07:00',
				'08:00',
				'13:00',
				'19:00',
				'morning',
				'afternoon',
				'evening',
			],
			'anyDay' => [
				'07:00',
				'08:00',
				'10:00',
				'11:00',
				'12:00',
				'13:00',
				'14:00',
				'15:00',
				'16:00',
				'17:00',
				'19:00',
				'morning',
				'afternoon',
				'evening',
			],
		];
		$actualEnabledDays = $this->getEnabledDays();
		assert(count($expectedDaysResult) === count($actualEnabledDays));
		assertType("array{'2019-01-04': array{'17:00', 'evening'}, '2019-01-05': array{'07:00', 'morning'}, '2019-01-06': array{'12:00', 'afternoon'}, '2019-01-07': array{'10:00', '11:00', '12:00', '13:00', '14:00', '15:00', '16:00', '17:00', 'morning', 'afternoon', 'evening'}, '2019-01-08': array{'07:00', '08:00', '13:00', '19:00', 'morning', 'afternoon', 'evening'}, anyDay: array{'07:00', '08:00', '10:00', '11:00', '12:00', '13:00', '14:00', '15:00', '16:00', '17:00', '19:00', 'morning', 'afternoon', 'evening'}}", $expectedDaysResult);
	}

	/**
	 * @return array<string, array<int, string>>
	 */
	private function getEnabledDays(): array
	{
		return [];
	}
}
