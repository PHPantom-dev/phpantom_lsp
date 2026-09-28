<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug11328;

use function PHPStan\Testing\assertType;

enum Status: string {
	case Live = 's1';
	case Expired = 's2';
}

/** @param Status[] $statuses */
function check(array $statuses): void {
	$fromDeadline = null;
	$toDeadline = null;
	if (in_array(Status::Live, $statuses, true)) {
		$fromDeadline = (new \DateTimeImmutable())->setTime(23, 59, 59);
	}
	if (in_array(Status::Expired, $statuses, true)) {
		assertType('DateTimeImmutable|null', $fromDeadline);
		if ($fromDeadline === null) {
			$toDeadline = (new \DateTimeImmutable())->setTime(0, 0, 0);
		}
	}
}
