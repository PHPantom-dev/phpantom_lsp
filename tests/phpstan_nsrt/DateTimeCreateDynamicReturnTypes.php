<?php declare(strict_types = 1);

namespace DateTimeCreateDynamicReturnTypes;

use function PHPStan\Testing\assertType;

class Foo
{

	public function createDynamic(string $datetime): void {
		assertType('DateTime|false', date_create($datetime));
		assertType('DateTimeImmutable|false', date_create_immutable($datetime));
	}

	public function staticInvalidDatetime(): void {
		assertType('false', date_create('202/20')); // SKIP: `date_create()` of a string it rejects is not folded to `false`
		assertType('false', date_create_immutable('202/20')); // SKIP: `date_create()` of a string it rejects is not folded to `false`
	}

	public function staticValidStrings(): void {
		assertType('DateTime', date_create('2020-10-12'));
		assertType('DateTimeImmutable', date_create_immutable('2020-10-12'));
	}

	public function localVariables(): void {
		$datetime = '2020-10-12';

		assertType('DateTime', date_create($datetime));
		assertType('DateTimeImmutable', date_create_immutable($datetime));
	}

	/**
	 * @param '2020-04-09'|'2022-02-01' $datetimes
	 * @param '1990-04-11'|'foo' $maybeDatetimes
	 * @param 'foo'|'bar' $noDatetimes
	 */
	public function unions(string $datetimes, string $maybeDatetimes, string $noDatetimes): void {
		assertType('DateTime', date_create($datetimes));
		assertType('DateTimeImmutable', date_create_immutable($datetimes));

		assertType('DateTime|false', date_create($maybeDatetimes));
		assertType('DateTimeImmutable|false', date_create_immutable($maybeDatetimes));

		assertType('false', date_create($noDatetimes)); // SKIP: `date_create()` of a string it rejects is not folded to `false`
		assertType('false', date_create_immutable($noDatetimes)); // SKIP: `date_create()` of a string it rejects is not folded to `false`
	}

}
