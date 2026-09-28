<?php

use function PHPStan\Testing\assertType;

$resource = fopen('php://memory', 'r');

if (is_resource($resource)) {
	assertType('resource', $resource);
} else {
	assertType('resource|false', $resource); // can be closed resource
}
