<?php declare(strict_types = 1);

namespace Bug13197b;

use function PHPStan\Testing\assertType;

function execute(string $command): void
{
	if (!function_exists('proc_open')) {
		return;
	}

	$pipes = [];

	$process = @proc_open(
		$command,
		[
			['pipe', 'rb'],
			3 => ['pipe', 'wb'], // https://stackoverflow.com/questions/28909347/is-it-possible-to-connect-more-than-the-two-standard-streams-to-a-terminal-in-li#28909376
			5 => ['pipe', 'wb'],
		],
		$pipes
	);

	assertType('array<0|3|5, resource>', $pipes);
}
