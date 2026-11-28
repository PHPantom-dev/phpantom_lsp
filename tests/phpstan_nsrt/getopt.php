<?php

namespace Getopt;

use function getopt;
use function PHPStan\Testing\assertType;

$opts = getopt("ab:c::", ["longopt1", "longopt2:", "longopt3::"], $restIndex);
assertType('(array<string, list<mixed>|string|false>|false)', $opts);
assertType('int<1, max>', $restIndex);
