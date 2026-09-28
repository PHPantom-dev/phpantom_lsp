<?php

namespace Bug5961;

use function PHPStan\Testing\assertType;

function doFoo() {
	assertType('"Foo \r\n Bar"', "Foo \r\n Bar");
	assertType('"Foo \n Bar"', "Foo \n Bar");
	assertType('"Foo \r Bar"', "Foo \r Bar");
}
