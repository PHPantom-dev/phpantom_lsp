<?php

namespace CaseInsensitivePhpDocs;

use function PHPStan\Testing\assertType;

use Foo\Bar;
use Foo\Baz as Lorem;

class Test
{

	/** @var bar */
	private $bar;

	/** @var lOREM */
	private $lorem;

	public function doFoo()
	{
		assertType('Foo\Bar', $this->bar);
		assertType('Foo\Baz', $this->lorem);
	}

}
