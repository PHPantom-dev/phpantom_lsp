<?php declare(strict_types=1);

namespace Bug2003;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

function (array $list): void {
	foreach ($list as $part) {
		switch (true) {
			case isset($list['magic']):
				$key = 'to-success';
				break;

			default:
				continue 2;
		}

		assertType('\'to-success\'', $key);
		assertVariableCertainty(TrinaryLogic::createYes(), $key);

		echo $key;
	}
};
