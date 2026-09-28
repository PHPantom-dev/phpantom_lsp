<?php

namespace Bug5920;

use function PHPStan\Testing\assertType;

use XMLReader;

function doFoo() {
	$reader = new XMLReader();
	$reader->open('');
	assertType('bool', $reader->read());
	while ($reader->read()) {}
	assertType('bool', $reader->read());
	$reader->close();
	$reader->open('');
	assertType('bool', $reader->read());
	while ($reader->read()) {}
	assertType('bool', $reader->read());
	$reader->close();
}
