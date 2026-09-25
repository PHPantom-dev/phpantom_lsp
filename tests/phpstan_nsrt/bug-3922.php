<?php

namespace Bug3922;

use function PHPStan\Testing\assertType;

/**
 * @template TResult
 * @template TQuery of QueryInterface<TResult>
 */
interface QueryHandlerInterface
{
	/**
	 * @param TQuery $query
	 *
	 * @return TResult
	 */
	public function handle(QueryInterface $query);
}

/**
 * @template TResult
 */
interface QueryInterface
{
}

/**
 * @template-implements QueryInterface<FooQueryResult>
 */
final class FooQuery implements QueryInterface
{
}

/**
 * @template-implements QueryInterface<BarQueryResult>
 */
final class BarQuery implements QueryInterface
{
}

final class FooQueryResult
{
}

final class BarQueryResult
{
}

/**
 * @template-implements QueryHandlerInterface<FooQueryResult, FooQuery>
 */
final class FooQueryHandler implements QueryHandlerInterface
{
	public function handle(QueryInterface $query)
	{
		return new FooQueryResult();
	}
}

function (FooQueryHandler $h): void {
	assertType(FooQueryResult::class, $h->handle(new FooQuery()));
	assertType(FooQueryResult::class, $h->handle(new BarQuery()));
};
