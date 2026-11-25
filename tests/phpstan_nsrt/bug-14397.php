<?php // lint >= 8.2

declare(strict_types = 1);

namespace Bug14397;

use PHPStan\TrinaryLogic;

use function filter_var;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

function test(string $ipAddress): void
{
	assertType('non-falsy-string|false', filter_var($ipAddress, FILTER_VALIDATE_IP, FILTER_FLAG_IPV4 | FILTER_FLAG_GLOBAL_RANGE));
}

function test2(mixed $mixed): void
{
	try {
		filter_var($mixed, FILTER_VALIDATE_INT, FILTER_FLAG_GLOBAL_RANGE);
		$foo = 1;
	} catch (\Filter\FilterFailedException $e) {
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}

	assertType('int|false', filter_var($mixed, FILTER_VALIDATE_INT, FILTER_FLAG_GLOBAL_RANGE));
	assertType('int|false', filter_var($mixed, FILTER_VALIDATE_INT, ['flags' => FILTER_FLAG_GLOBAL_RANGE]));
}
