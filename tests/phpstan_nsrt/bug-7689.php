<?php

namespace Bug7689;

use \XMLReader;
use function PHPStan\Testing\assertType;

class Reader extends XMLReader
{
}

function keyValue(Reader $reader, string $namespace = null): array
{
	// If there's no children, we don't do anything.
	if ($reader->isEmptyElement) {
		$reader->next();

		return [];
	}

	if (!$reader->read()) {
		$reader->next();

		return [];
	}

	if (Reader::END_ELEMENT === $reader->nodeType) {
		$reader->next();

		return [];
	}

	$values = [];

	do {
		if (Reader::ELEMENT === $reader->nodeType) {

		} else {
			assertType('bool', $reader->read());
			if (!$reader->read()) {
				break;
			}
		}
	} while (Reader::END_ELEMENT !== $reader->nodeType);

	assertType('bool', $reader->read());
	$reader->read();

	return $values;
}
