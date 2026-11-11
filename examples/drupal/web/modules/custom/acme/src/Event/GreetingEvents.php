<?php

declare(strict_types=1);

namespace Drupal\acme\Event;

/**
 * The events the module dispatches, named once for its subscribers.
 */
final class GreetingEvents
{
    /**
     * Dispatched once a visitor has been greeted.
     */
    public const GREETED = 'acme.greeted';
}
