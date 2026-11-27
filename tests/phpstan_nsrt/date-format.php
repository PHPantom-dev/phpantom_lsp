<?php

namespace DateFormatReturnType;

use function PHPStan\Testing\assertType;

function (string $s): void {
	assertType('\'\'', date(''));
	assertType('string', date($s));
	assertType('non-falsy-string', date('D'));
	assertType('numeric-string', date('Y'));
	assertType('numeric-string', date('Ghi'));
};

function (\DateTime $dt, string $s): void {
	assertType('\'\'', date_format($dt, ''));
	assertType('string', date_format($dt, $s));
	assertType('non-falsy-string', date_format($dt, 'D'));
	assertType('numeric-string', date_format($dt, 'Y'));
	assertType('numeric-string', date_format($dt, 'Ghi'));
};

function (\DateTimeInterface $dt, string $s): void {
	assertType('\'\'', $dt->format(''));
	assertType('string', $dt->format($s));
	assertType('non-falsy-string', $dt->format('D'));
	assertType('numeric-string', $dt->format('Y'));
	assertType('numeric-string', $dt->format('Ghi'));
};

function (\DateTime $dt, string $s): void {
	assertType('\'\'', $dt->format(''));
	assertType('string', $dt->format($s));
	assertType('non-falsy-string', $dt->format('D'));
	assertType('numeric-string', $dt->format('Y'));
	assertType('numeric-string', $dt->format('Ghi'));
};

function (\DateTimeImmutable $dt, string $s): void {
	assertType('\'\'', $dt->format(''));
	assertType('string', $dt->format($s));
	assertType('non-falsy-string', $dt->format('D'));
	assertType('numeric-string', $dt->format('Y'));
	assertType('numeric-string', $dt->format('Ghi'));
};

function (?\DateTimeImmutable $d): void {
	assertType('DateTimeImmutable', $d->modify('+1 day')); // PHPStan says `DateTimeImmutable|null`, but calling a method on `null` throws
};
