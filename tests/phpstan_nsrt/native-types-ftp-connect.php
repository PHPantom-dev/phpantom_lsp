<?php // lint >= 8.1

namespace NativeTypesFtpConnect;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(): void
	{
		$f = ftp_connect('example.com');
		assertType('FTP\Connection|false', $f);
		assertNativeType('FTP\Connection|false', $f);
	}

}
