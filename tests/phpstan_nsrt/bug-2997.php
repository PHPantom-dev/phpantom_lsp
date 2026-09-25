<?php

namespace Bug2997;

use function PHPStan\Testing\assertType;

function (\SimpleXMLElement $xml): void {
	assertType('bool', (bool) $xml->Exists);
};
