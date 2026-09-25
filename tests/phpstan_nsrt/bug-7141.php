<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug7141;

use stdClass;
use function PHPStan\Testing\assertType;

interface Container
{
    /**
     * @template T
     * @return ($id is class-string<T> ? T : mixed)
     */
    public function get(string $id): mixed;
}


function(Container $c) {
	assertType('mixed', $c->get('test'));
	assertType('stdClass', $c->get(stdClass::class));
};
