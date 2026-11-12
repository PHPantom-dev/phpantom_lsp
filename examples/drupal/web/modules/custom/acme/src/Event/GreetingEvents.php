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
     *
     * GreetingSubscriber keys its map by this constant, so it defines the
     * event 'acme.greeted'. Try: find the references of GREETED, or of the
     * string it holds: the key of GreetingSubscriber and the dispatch of
     * 'acme.greeted' in acme.module are listed with it. Renaming GREETED
     * leaves that string alone.
     */
    public const GREETED = 'acme.greeted';
}
