<?php

namespace Bug14136Nsrt;

use function PHPStan\Testing\assertType;

$xml = <<<'EOT'
<?xml version="1.0"?><item>3</item>
EOT;

$xml = simplexml_load_string($xml);
assertType('int', intval($xml));
assertType('int', intval(gmp_init(42)));
assertType('int', (int) ($xml));
assertType('int', (int) (gmp_init(42)));

assertType('float', floatval($xml));
assertType('float', floatval(gmp_init(42)));
assertType('float', (float) ($xml));
assertType('float', (float) (gmp_init(42)));

assertType('string', strval($xml));
assertType('string', (string) ($xml));

assertType('bool', boolval($xml));
assertType('bool', boolval(gmp_init(0)));
assertType('bool', (bool) ($xml));
assertType('bool', (bool) (gmp_init(0)));
