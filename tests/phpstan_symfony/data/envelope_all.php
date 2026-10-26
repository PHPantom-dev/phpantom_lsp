<?php declare(strict_types = 1);

use function PHPStan\Testing\assertType;

$envelope = new \Symfony\Component\Messenger\Envelope(new stdClass());

assertType('list<Symfony\Component\Messenger\Stamp\ReceivedStamp>', $envelope->all(\Symfony\Component\Messenger\Stamp\ReceivedStamp::class));
assertType('list<Symfony\Component\Messenger\Stamp\StampInterface>', $envelope->all(random_bytes(1)));
assertType('array<class-string<Symfony\Component\Messenger\Stamp\StampInterface>, list<Symfony\Component\Messenger\Stamp\StampInterface>>', $envelope->all());
