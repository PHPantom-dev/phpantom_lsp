<?php declare(strict_types = 1);

namespace Bug13197;

use function PHPStan\Testing\assertType;

/**
 * @return array{string, string}|null STDOUT & STDERR tuple
 */
function execute(string $command): ?array
{
	if (!function_exists('proc_open')) {
		return null;
	}

	$pipes = [];

	$process = @proc_open(
		$command,
		[
			['pipe', 'rb'],
			['pipe', 'wb'], // stdout
			['pipe', 'wb'], // stderr
		],
		$pipes
	);

	assertType('array<0|1|2, resource>', $pipes);

	if (!is_resource($process)) {
		return null;
	}

	fclose($pipes[0]);

	$stdout = (string) stream_get_contents($pipes[1]);
	$stderr = (string) stream_get_contents($pipes[2]);

	proc_close($process);

	return [$stdout, $stderr];
}
