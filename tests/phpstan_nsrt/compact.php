<?php

namespace CompactExtension;

use function PHPStan\Testing\assertType;

assertType('array{bar?: mixed}', compact(['foo' => 'bar']));

function (string $dolor): void {
	$foo = 'bar';
	$bar = 'baz';
	if (rand(0, 1)) {
		$lorem = 'ipsum';
	}
	assertType('array{foo: \'bar\', bar: \'baz\'}', compact('foo', ['bar']));
	assertType('array{foo: \'bar\', bar: \'baz\', lorem?: \'ipsum\'}', compact([['foo']], 'bar', 'lorem')); // SKIP: a variable assigned on only one branch is not known to be maybe-undefined

	assertType('array<string, mixed>', compact($dolor));
	assertType('array<string, mixed>', compact([$dolor]));

	assertType('array{}', compact([]));
};
