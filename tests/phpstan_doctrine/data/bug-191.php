<?php

namespace Bug191;

use Doctrine\ORM\Query\Expr\Func;
use Doctrine\ORM\Query\Expr\Literal;
use function PHPStan\Testing\assertType;

class Foo
{

	public function getTopicConcatExpression(): void
	{
		$func = new Func('CONCAT_WS', [
			new Literal("'.'"),
			'event.type.businessDomain',
			'event.type.group',
			'event.type.resource',
			'event.type.subResource',
			'event.type.action',
			'event.type.version',
		]);
		assertType(Func::class, $func);
	}

}
