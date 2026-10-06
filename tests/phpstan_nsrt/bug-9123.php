<?php declare(strict_types=1);

namespace Bug9123;

interface Event {}

class MyEvent implements Event {}

/** @template T of Event */
interface EventListener
{
	/** @phpstan-assert-if-true T $event */
	public function canBeListen(Event $event): bool;

	public function listen(Event $event): void;
}

/** @implements EventListener<MyEvent> */
final class Implementation implements EventListener
{
	public function canBeListen(Event $event): bool
	{
		return $event instanceof MyEvent;
	}

	public function listen(Event $event): void
	{
		if (! $this->canBeListen($event)) {
			return;
		}

		\PHPStan\Testing\assertType('Bug9123\MyEvent', $event);
	}
}

/** @implements EventListener<MyEvent> */
final class Implementation2 implements EventListener
{
	/** @phpstan-assert-if-true MyEvent $event */
	public function canBeListen(Event $event): bool
	{
		return $event instanceof MyEvent;
	}

	public function listen(Event $event): void
	{
		if (! $this->canBeListen($event)) {
			return;
		}

		\PHPStan\Testing\assertType('Bug9123\MyEvent', $event);
	}
}
