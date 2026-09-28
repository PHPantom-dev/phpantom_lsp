<?php declare(strict_types = 1);

namespace Bug9963;

use function PHPStan\Testing\assertType;

class HelloWorld
{
    /**
     * @param int|int[]       $id
     *
     * @return ($id is array ? static[] : static|false)
     */
    final public function find($id)
    {
		return [];
	}
}

function ($something) {
	$h = new HelloWorld();
	assertType('array<Bug9963\HelloWorld>|Bug9963\HelloWorld|false', $h->find($something));
};
