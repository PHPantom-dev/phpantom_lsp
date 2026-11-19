<?php

declare(strict_types=1);

namespace Drupal\acme\EventSubscriber;

use Symfony\Component\EventDispatcher\EventSubscriberInterface;
use Symfony\Component\HttpKernel\Event\RequestEvent;
use Symfony\Component\HttpKernel\KernelEvents;

/**
 * Marks every request as greeted.
 *
 * It returns its map as a literal.
 */
final class RequestSubscriber implements EventSubscriberInterface
{
    public static function getSubscribedEvents(): array
    {
        // Try: go to the definition of 'onRequest', or find the references
        // of onRequest(): this string is one of them.
        return [KernelEvents::REQUEST => ['onRequest', 50]];
    }

    public function onRequest(RequestEvent $event): void
    {
        $event->getRequest()->attributes->set('_acme_greeted', true);
    }
}
