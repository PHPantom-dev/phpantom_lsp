<?php declare(strict_types = 1);

use Symfony\Component\Messenger\TraceableMessageBus;
use function PHPStan\Testing\assertType;

$bus = new TraceableMessageBus(new class implements \Symfony\Component\Messenger\MessageBusInterface {
	public function dispatch(object $message, array $stamps = []): \Symfony\Component\Messenger\Envelope
	{
		return new \Symfony\Component\Messenger\Envelope($message);
	}
});

$messages = $bus->getDispatchedMessages();
assertType('list<array{stamps: list<Symfony\Component\Messenger\Stamp\StampInterface>, message: object, caller: array{name: string, file: string|null, line: int|null}, callTime: float, exception?: Throwable, stamps_after_dispatch: list<Symfony\Component\Messenger\Stamp\StampInterface>}>', $messages); // SKIP: Symfony return types an argument decides
