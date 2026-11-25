<?php declare(strict_types = 1);

namespace Bug12954;

use function PHPStan\Testing\assertType;

$plop = [
	12 => [
		'name' => 'ROLE_USER',
		'description' => 'User role'
	],
	28 => [
		'name' => 'ROLE_ADMIN',
		'description' => 'Admin role'
	],
	43 => [
		'name' => 'ROLE_SUPER_ADMIN',
		'description' => 'SUPER Admin role'
	],
];

$list = ['ROLE_USER', 'ROLE_ADMIN', 'ROLE_SUPER_ADMIN'];

$result = array_column($plop, 'name', null);

/**
 * @param list<string> $array
 */
function doSomething(array $array): void
{
	assertType('list<string>', $array);
}

doSomething($result);
doSomething($list);

assertType('array{\'ROLE_USER\', \'ROLE_ADMIN\', \'ROLE_SUPER_ADMIN\'}', $result);
assertType('array{\'ROLE_USER\', \'ROLE_ADMIN\', \'ROLE_SUPER_ADMIN\'}', $list);
