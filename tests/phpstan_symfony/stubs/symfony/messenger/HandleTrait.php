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

use Symfony\Component\Messenger\Exception\LogicException;
use Symfony\Component\Messenger\Stamp\HandledStamp;
use Symfony\Component\Messenger\Stamp\StampInterface;

/**
 * Leverages a message bus to expect a single, synchronous message handling and return its result.
 *
 * @author Maxime Steinhausser <maxime.steinhausser@gmail.com>
 */
trait HandleTrait
{
    private MessageBusInterface $messageBus;

    /**
     * Dispatches the given message, expecting to be handled by a single handler
     * and returns the result from the handler returned value.
     * This behavior is useful for both synchronous command & query buses,
     * the last one usually returning the handler result.
     *
     * @param object|Envelope  $message The message or the message pre-wrapped in an envelope
     * @param StampInterface[] $stamps  Stamps to be set on the Envelope which are used to control middleware behavior
     */
    private function handle(object $message, array $stamps = []): mixed
    {}
}
