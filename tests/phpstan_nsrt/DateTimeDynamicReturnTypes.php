<?php declare(strict_types = 1);

namespace DateTimeDynamicReturnTypes;

use DateTime;
use DateTimeImmutable;
use function date_create_from_format;
use function PHPStan\Testing\assertType;

class Foo
{

	public function createDynamic(string $format, string $datetime): void {
		assertType('DateTime|false', date_create_from_format($format, $datetime));
		assertType('DateTimeImmutable|false', date_create_immutable_from_format($format, $datetime));
	}

	public function staticInvalidFormat(): void {
		assertType('false', date_create_from_format('Foobar', '2022-02-20'));
		assertType('false', date_create_immutable_from_format('Foobar', '2022-02-20'));
	}

	public function staticInvalidDatetime(): void {
		assertType('false', date_create_from_format('Y-m-d', '2022/02/20'));
		assertType('false', date_create_immutable_from_format('Y-m-d', '2022/02/20'));
	}

	public function staticValidStrings(): void {
		assertType('DateTime', date_create_from_format('Y-m-d', '2020-10-12'));
		assertType('DateTimeImmutable', date_create_immutable_from_format('Y-m-d', '2020-10-12'));
	}

	public function localVariables(): void {
		$format = 'Y-m-d';
		$datetime = '2020-10-12';

		assertType('DateTime', date_create_from_format($format, $datetime));
		assertType('DateTimeImmutable', date_create_immutable_from_format($format, $datetime));
	}

	/**
	 * @param '2020-04-09'|'2022-02-01' $datetimes
	 * @param '1990-04-11'|'foo' $maybeDatetimes
	 * @param 'foo'|'bar' $noDatetimes
	 */
	public function unions(string $datetimes, string $maybeDatetimes, string $noDatetimes): void {
		assertType('DateTime', date_create_from_format('Y-m-d', $datetimes));
		assertType('DateTimeImmutable', date_create_immutable_from_format('Y-m-d', $datetimes));

		assertType('DateTime|false', date_create_from_format('Y-m-d', $maybeDatetimes));
		assertType('DateTimeImmutable|false', date_create_immutable_from_format('Y-m-d', $maybeDatetimes));

		assertType('false', date_create_from_format('Y-m-d', $noDatetimes));
		assertType('false', date_create_immutable_from_format('Y-m-d', $noDatetimes));
	}

}
