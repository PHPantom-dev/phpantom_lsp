<?php

namespace Bug1945;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

function (): void {
	foreach (["a", "b", "c"] as $letter) {
		switch ($letter) {
			case "b":
				$foo = 1;
				break;
			case "c":
				$foo = 2;
				break;
			default:
				continue 2;
		}

		assertType('1|2', $foo);
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function (): void {
	foreach (["a", "b", "c"] as $letter) {
		switch ($letter) {
			case "a":
				if (rand(0, 10) === 1) {
					continue 2;
				}
				$foo = 1;
				break;
			case "b":
				if (rand(0, 10) === 1) {
					continue 2;
				}
				$foo = 2;
				break;
			default:
				continue 2;
		}

		assertType('1|2', $foo);
		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
	}
};

function (array $docs): void {
	foreach ($docs as $doc) {
		switch (true) {
			case 'bar':
				continue 2;
				break;
			default:
				$foo = $doc;
				break;
		}

		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
		if (!$foo) {
			return;
		}
	}
};

function (array $docs): void {
	foreach ($docs as $doc) {
		switch (true) {
			case 'bar':
				continue 2;
			default:
				$foo = $doc;
				break;
		}

		assertVariableCertainty(TrinaryLogic::createYes(), $foo);
		if (!$foo) {
			return;
		}
	}
};

function (array $items): string {
	foreach ($items as $item) {
		switch ($item) {
			case 1:
				$string = 'a';
				break;
			case 2:
				$string = 'b';
				break;
			default:
				continue 2;
		}

		assertType('\'a\'|\'b\'', $string);
		assertVariableCertainty(TrinaryLogic::createYes(), $string);

		return 'result: ' . $string;
	}

	return 'ok';
};
