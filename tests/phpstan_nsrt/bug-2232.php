<?php

namespace Bug2232;

use function PHPStan\Testing\assertType;

function () {
	$data = [
		'a1' => "a",
		'a2' => "b",
		'a3' => "c",
		'a4' => [
			'name' => "dsfs",
			'version' => "fdsfs",
		],
	];

	if (rand(0, 1)) {
		$data['b1'] = "hello";
	}

	if (rand(0, 1)) {
		$data['b2'] = "hello";
	}

	if (rand(0, 1)) {
		$data['b3'] = "hello";
	}

	if (rand(0, 1)) {
		$data['b4'] = "goodbye";
	}

	if (rand(0, 1)) {
		$data['b5'] = "env";
	}

	assertType('array{a1: \'a\', a2: \'b\', a3: \'c\', a4: array{name: \'dsfs\', version: \'fdsfs\'}, b1?: \'hello\', b2?: \'hello\', b3?: \'hello\', b4?: \'goodbye\', b5?: \'env\'}', $data);
};
