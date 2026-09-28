<?php

namespace Bug5365;

use function PHPStan\Testing\assertType;

function (): void {
	$matches = [];
	$pattern = '#^C\s+(?<productId>\d+)$#i';
	$subject = 'C 1234567890';

	$found = (bool)preg_match( $pattern, $subject, $matches ) && isset( $matches['productId'] );
	assertType('bool', $found);
};

function (): void {
	$matches = [];
	$pattern = '#^C\s+(?<productId>\d+)$#i';
	$subject = 'C 1234567890';

	assertType('bool', preg_match( $pattern, $subject, $matches ) ? isset( $matches['productId'] ) : false);
};
