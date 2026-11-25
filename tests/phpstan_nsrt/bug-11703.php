<?php declare(strict_types = 1);

namespace Bug11703;

use function PHPStan\Testing\assertType;

function test(int|string|null $duration, string $foo): ?bool
{
	$alerts = [];

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert.',
			'duration' => $duration,
			'severity' => 100,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert2.',
			'duration' => $duration,
			'severity' => 99,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert3.',
			'duration' => $duration,
			'severity' => 75,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert4.',
			'duration' => $duration,
			'severity' => 60,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert5.',
			'duration' => null,
			'severity' => 25,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert6.',
			'duration' => $duration,
			'severity' => 24,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert7.',
			'duration' => $duration,
			'severity' => 24,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert8.',
			'duration' => $duration,
			'severity' => 24,
		];
	}

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert9.',
			'duration' => $duration,
			'severity' => 24,
		];
	}

	assertType('int<0, 9>', count($alerts));

	if (mt_rand(0, 1)) {
		$alerts[] = [
			'message'  => 'Some message about the alert10.',
			'duration' => $duration,
			'severity' => 23,
		];
	}

	if (count($alerts) === 0) {
		return null;
	}

	return true;
}
