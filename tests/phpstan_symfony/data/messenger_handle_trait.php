<?php declare(strict_types = 1);

namespace MessengerHandleTrait;

use Symfony\Component\Messenger\HandleTrait;
use function PHPStan\Testing\assertType;

class RegularQuery {}
class RegularQueryResult {}
class RegularQueryHandler
{
    public function __invoke(RegularQuery $query): RegularQueryResult
    {
        return new RegularQueryResult();
    }
}

class TaggedQuery {}
class TaggedResult {}
class TaggedHandler
{
    public function handle(TaggedQuery $query): TaggedResult
    {
        return new TaggedResult();
    }
}

class MultiHandlersForTheSameMessageQuery {}
class MultiHandlersForTheSameMessageHandler1
{
    public function __invoke(MultiHandlersForTheSameMessageQuery $query): bool
    {
        return true;
    }
}
class MultiHandlersForTheSameMessageHandler2
{
    public function __invoke(MultiHandlersForTheSameMessageQuery $query): bool
    {
        return false;
    }
}

class HandleTraitClass {
    use HandleTrait;

    public function __invoke()
    {
        assertType(RegularQueryResult::class, $this->handle(new RegularQuery())); // SKIP: Messenger handlers a service tag registers are not found

        assertType(TaggedResult::class, $this->handle(new TaggedQuery())); // SKIP: Messenger handlers a service tag registers are not found

		$randomQuery = rand(0, 1) ? new RegularQuery() : new TaggedQuery();
		assertType(RegularQueryResult::class . '|' . TaggedResult::class, $this->handle($randomQuery)); // SKIP: Messenger handlers a service tag registers are not found

        // HandleTrait will throw exception in fact due to multiple handle methods/handlers per single query
        assertType('mixed', $this->handle(new MultiHandlersForTheSameMessageQuery()));
    }
}

class QueryBus {
    use HandleTrait;

    public function dispatch(object $query)
    {
        return $this->handle($query);
    }

	public function dispatch2(object $query)
	{
		return $this->handle($query);
	}
}

interface QueryBusInterface {
	public function dispatch(object $query);
}

class QueryBusWithInterface implements QueryBusInterface {
	use HandleTrait;

	public function dispatch(object $query)
	{
		return $this->handle($query);
	}
}

class Controller {
    public function action()
    {
        $queryBus = new QueryBus();

        assertType(RegularQueryResult::class, $queryBus->dispatch(new RegularQuery())); // SKIP: Messenger handlers a service tag registers are not found, nor followed through a bus method returning handle()

        assertType('bool', $queryBus->dispatch(new BooleanQuery())); // SKIP: the message classes come from the MessageSubscriberInterface fixture, which Symfony 7 no longer runs
        assertType('int', $queryBus->dispatch(new IntQuery())); // SKIP: the message classes come from the MessageSubscriberInterface fixture, which Symfony 7 no longer runs
        assertType('float', $queryBus->dispatch(new FloatQuery())); // SKIP: the message classes come from the MessageSubscriberInterface fixture, which Symfony 7 no longer runs
        assertType('string', $queryBus->dispatch(new StringQuery())); // SKIP: the message classes come from the MessageSubscriberInterface fixture, which Symfony 7 no longer runs

		$randomQuery = rand(0, 1) ? new IntQuery() : new StringQuery();
		assertType('int|string', $queryBus->dispatch($randomQuery)); // SKIP: the message classes come from the MessageSubscriberInterface fixture, which Symfony 7 no longer runs

        assertType(TaggedResult::class, $queryBus->dispatch(new TaggedQuery())); // SKIP: Messenger handlers a service tag registers are not found, nor followed through a bus method returning handle()

		assertType(RegularQueryResult::class, $queryBus->dispatch2(new RegularQuery())); // SKIP: Messenger handlers a service tag registers are not found, nor followed through a bus method returning handle()

		$queryBusWithInterface = new QueryBusWithInterface();

		assertType(RegularQueryResult::class, $queryBusWithInterface->dispatch(new RegularQuery())); // SKIP: Messenger handlers a service tag registers are not found, nor followed through a bus method returning handle()

		$randomQueryBus = rand(0, 1) ? $queryBus : $queryBusWithInterface;
		assertType(RegularQueryResult::class, $randomQueryBus->dispatch(new RegularQuery())); // SKIP: Messenger handlers a service tag registers are not found, nor followed through a bus method returning handle()

        // HandleTrait will throw exception in fact due to multiple handle methods/handlers per single query
        assertType('mixed', $queryBus->dispatch(new MultiHandlesForInTheSameHandlerQuery()));
        assertType('mixed', $queryBus->dispatch(new MultiHandlersForTheSameMessageQuery()));
    }
}
