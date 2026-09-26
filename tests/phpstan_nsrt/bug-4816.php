<?php

namespace Bug4816;

use function PHPStan\Testing\assertType;

function (): void {
	if (is_dir('foo')) {
		assertType('true', is_dir('foo'));
		assertType('bool', is_dir('bar'));

		clearstatcache();

		assertType('bool', is_dir('foo'));
		assertType('bool', is_dir('bar'));
	}
};

function (): void {
	if (!is_dir('foo')) {
		// More precise than upstream, which reports `bool`: the failed check is
		// remembered the way a passed one is, until the stat cache is cleared.
		assertType('false', is_dir('foo'));
		assertType('bool', is_dir('bar'));
	}
};

function (): void {
	if (!is_dir('foo')) {
		return;
	}

	assertType('true', is_dir('foo'));
	assertType('bool', is_dir('bar'));

	clearstatcache();

	assertType('bool', is_dir('foo'));
	assertType('bool', is_dir('bar'));
};

function (): void {
	if (is_dir('foo')) {
		return;
	}

	// More precise than upstream, which reports `bool`: the failed check is
	// remembered the way a passed one is, until the stat cache is cleared.
	assertType('false', is_dir('foo'));
	assertType('bool', is_dir('bar'));
};
