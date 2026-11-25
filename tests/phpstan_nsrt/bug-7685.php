<?php // lint >= 8.0

namespace bug7685;

use function PHPStan\Testing\assertType;

interface Reader {
	public function getFilePath(): string|false;
}

function bug7685(Reader $reader): void {
	$filePath = $reader->getFilePath();
	if (false !== (bool) $filePath) {
		assertType('non-falsy-string', $filePath);
	}
}

