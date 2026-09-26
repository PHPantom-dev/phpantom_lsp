<?php

namespace ClearStatCache;

use function clearstatcache;
use function PHPStan\Testing\assertType;

function (string $a, string $b, bool $c): string {
	if (is_file($a) && $c) {
		assertType('true', is_file($a));
		assertType('bool', is_file($b));
		assertType('true', $c);
		clearstatcache();
		assertType('bool', is_file($a));
		assertType('bool', is_file($b));
		assertType('true', $c);
	}
};

function (string $a, string $b, bool $c): string {
	if (\is_file($a) && $c) {
		//assertType('true', is_file($a));
		assertType('bool', is_file($b));
		assertType('true', $c);
		clearstatcache();
		assertType('bool', is_file($a));
		assertType('bool', is_file($b));
		assertType('true', $c);
	}
};

function (string $a, string $b, bool $c): string {
	if (is_file($a) && $c) {
		//assertType('true', \is_file($a));
		assertType('bool', \is_file($b));
		assertType('true', $c);
		clearstatcache();
		assertType('bool', \is_file($a));
		assertType('bool', \is_file($b));
		assertType('true', $c);
	}
};

function (string $a, string $b, bool $c): string {
	if (\is_file($a) && $c) {
		assertType('true', \is_file($a));
		assertType('bool', \is_file($b));
		assertType('true', $c);
		clearstatcache();
		assertType('bool', \is_file($a));
		assertType('bool', \is_file($b));
		assertType('true', $c);
	}
};

function (): void {
	if (file_exists('foo')) {
		assertType('true', file_exists('foo'));
		unlink('foo');
		assertType('bool', file_exists('foo'));
	}
};
