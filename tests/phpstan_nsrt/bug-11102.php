<?php declare(strict_types = 1);

namespace Bug11102;

use DateTime;
use function PHPStan\Testing\assertType;

/** @var array{'start': DateTime|null} $details */
$details = ['start' => null];

$startIsADateTime = $details['start'] instanceof DateTime;

if ($startIsADateTime) {
	assertType('DateTime', $details['start']);
}
