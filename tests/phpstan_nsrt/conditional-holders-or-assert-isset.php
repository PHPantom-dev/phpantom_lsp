<?php declare(strict_types=1);

namespace ConditionalHoldersOrAssertIsset;

use function PHPStan\Testing\assertType;

/**
 * @return list<int>
 */
function getListOfIds(): array {
	return [3];
}

/**
 * @phpstan-impure
 */
function getBool(): bool {
	return (bool) rand(0, 1);
}

$displayDetails = getBool();
$displayList = getBool();

if ($displayList || $displayDetails) {
	$listOfIds = getListOfIds();
}

if ($displayList) {
	assert(isset($listOfIds));
}

if ($displayDetails) {
	assert(isset($listOfIds));
	assertType('bool', $displayList);
}
