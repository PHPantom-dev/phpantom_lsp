<?php

namespace ProcGetStatusBug;

use function PHPStan\Testing\assertType;
use function proc_get_status;

function ($r): void {
	$status = proc_get_status($r);
	if ($status === false) {
		return;
	}

	assertType('array{command: string, pid: int, running: bool, signaled: bool, stopped: bool, exitcode: int, termsig: int, stopsig: int}', $status);
};
