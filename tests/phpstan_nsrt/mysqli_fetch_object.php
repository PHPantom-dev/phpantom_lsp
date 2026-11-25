<?php

namespace MysqliFetchObject;

use function PHPStan\Testing\assertType;

function doFoo(\mysqli_result $result) {
	assertType('stdClass|false|null', $result->fetch_object());
	assertType('MysqliFetchObject\MyClass|false|null', $result->fetch_object(MyClass::class));

	assertType('stdClass|false|null', mysqli_fetch_object($result));
	assertType('MysqliFetchObject\MyClass|false|null', mysqli_fetch_object($result, MyClass::class));
}

class MyClass {}

