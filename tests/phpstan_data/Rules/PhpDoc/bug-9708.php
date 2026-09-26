<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug9708;

use function PHPStan\Testing\assertType;

interface EventTopic {

}

enum FooEventTopic: string implements EventTopic {
	case Foo = 'foo';
}

/**
 * @template TEventTopic of EventTopic
 */
interface EventType {

}


/**
 * @template-implements EventType<FooEventTopic>
 */
enum FooEventType: string implements EventType {
	case Foo = 'foo';
}

/**
 * @template TEventTopic of EventTopic
 * @template TEventType of EventType<TEventTopic>
 */
interface Event {

}

/**
 * @template-implements Event<FooEventTopic, FooEventType>
 */
abstract class FooEvent implements Event {

}

/**
 * This works as expected and is sufficient for this use-case
 *
 * @template TEvent of Event<EventTopic, EventType<EventTopic>>
 */
class EventDispatcher {

	/**
	 * @param TEvent $bar
	 */
	public function dispatch (Event $bar): void {
		assertType('TEvent of Bug9708\Event<Bug9708\EventTopic, Bug9708\EventType<Bug9708\EventTopic>> (class Bug9708\EventDispatcher, argument)', $bar);
	}
}

/**
 * The generic parameter must be specified twice! Even though it's part of the template declaration
 *
 * @template TEventTopic of EventTopic
 * @template TEventType of EventType<TEventTopic>
 * @template TEvent of Event<TEventTopic, TEventType<TEventTopic>>
 */
class EventDispatcher2 {

	/**
	 * @param TEvent $bar
	 */
	public function dispatch (Event $bar): void {
		assertType('TEvent of Bug9708\Event<TEventTopic of Bug9708\EventTopic (class Bug9708\EventDispatcher2, argument), Bug9708\EventType<TEventTopic of Bug9708\EventTopic (class Bug9708\EventDispatcher2, argument)>> (class Bug9708\EventDispatcher2, argument)', $bar);
	}
}

/**
 * This should work but doesn't
 *
 * @template TEventTopic of EventTopic
 * @template TEventType of EventType<TEventTopic>
 * @template TEvent of Event<TEventTopic, TEventType>
 */
class EventDispatcher3 {

	/**
	 * @param TEvent $bar
	 */
	public function dispatch (Event $bar): void {
		assertType('TEvent of Bug9708\Event<TEventTopic of Bug9708\EventTopic (class Bug9708\EventDispatcher3, argument), TEventType of Bug9708\EventType<TEventTopic of Bug9708\EventTopic (class Bug9708\EventDispatcher3, argument)> (class Bug9708\EventDispatcher3, argument)> (class Bug9708\EventDispatcher3, argument)', $bar);
	}
}
