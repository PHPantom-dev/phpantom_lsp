<?php

namespace Bug4816;

use function PHPStan\Testing\assertType;

function (): void {
	if (is_dir('foo')) {
		assertType('true', is_dir('foo'));
		assertType('bool', is_dir('bar'));

		clearstatcache();

		assertType('bool', is_dir('foo')); // SKIP: a narrowed member read or call result survives a call that can change it
		assertType('bool', is_dir('bar'));
	}
};

function (): void {
	if (!is_dir('foo')) {
		assertType('bool', is_dir('foo')); // SKIP: a narrowed member read or call result survives a call that can change it
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

	assertType('bool', is_dir('foo')); // SKIP: a narrowed member read or call result survives a call that can change it
	assertType('bool', is_dir('bar'));
};

function (): void {
	if (is_dir('foo')) {
		return;
	}

	assertType('bool', is_dir('foo')); // SKIP: a narrowed member read or call result survives a call that can change it
	assertType('bool', is_dir('bar'));
};
