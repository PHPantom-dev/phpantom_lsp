<?php

namespace Analyser\JsonDecode;

use function PHPStan\Testing\assertType;

$value = json_decode('{"key"}');
assertType('null', $value);

$value = json_decode('{"key"}', true);
assertType('null', $value);

$value = json_decode('{"key"}', null);
assertType('null', $value);

$value = json_decode('{"key"}', false);
assertType('null', $value);
