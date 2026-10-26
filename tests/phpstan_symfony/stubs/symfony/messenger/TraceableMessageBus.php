<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Messenger;

/**
 * @author Samuel Roze <samuel.roze@gmail.com>
 */
class TraceableMessageBus implements MessageBusInterface
{
    private array $dispatchedMessages = [];

    public function __construct(
        private MessageBusInterface $decoratedBus,
        protected readonly ?\Closure $disabled = null,
    ) {}

    public function dispatch(object $message, array $stamps = []): Envelope
    {}

    public function getDispatchedMessages(): array
    {}

    public function reset(): void
    {}

    private function getCaller(): array
    {}
}
