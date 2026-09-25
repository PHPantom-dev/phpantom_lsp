<?php

use function PHPStan\Testing\assertType;

const TABLE_NAME = 'resized_images';
define('ANOTHER_NAME', 'foo');

assertType('\'resized_images\'', TABLE_NAME);
assertType('\'resized_images\'', \TABLE_NAME);
assertType('\'foo\'', ANOTHER_NAME);
assertType('\'foo\'', \ANOTHER_NAME);

function () {
	assertType('\'resized_images\'', TABLE_NAME);
	assertType('\'resized_images\'', \TABLE_NAME);
	assertType('\'foo\'', ANOTHER_NAME);
	assertType('\'foo\'', \ANOTHER_NAME);
};
