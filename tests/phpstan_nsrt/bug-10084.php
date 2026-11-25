<?php

namespace Bug10084;

use function PHPStan\Testing\assertType;

function (?int $val) {
	match ($val) {
		null => assertType('null', $val),
		default => assertType('int', $val),
	};
};

function (?int $val) {
	match ($foo = $val) {
		null => assertType('null', $val), // SKIP: `match ($foo = $val)` narrows `$foo` but not `$val`
		default => assertType('int', $val), // SKIP: `match ($foo = $val)` narrows `$foo` but not `$val`
	};
};

function (?int $val) {
	match ($foo = $val) {
		null => assertType('null', $foo),
		default => assertType('int', $foo),
	};
};
