<?php

declare(strict_types=1);

namespace Drupal\acme\EventSubscriber;

use Drupal\Core\Config\ConfigCrudEvent;
use Drupal\Core\Config\ConfigEvents;
use Symfony\Component\EventDispatcher\EventSubscriberInterface;
use Symfony\Component\HttpKernel\Event\ResponseEvent;
use Symfony\Component\HttpKernel\KernelEvents;

/**
 * Greets every response, and notices when the greeting changes.
 *
 * It builds its map by assignment, the way most of core's subscribers do.
 */
final class GreetingSubscriber implements EventSubscriberInterface
{
    /** Whether acme.settings was saved during this request. */
    public bool $greetingChanged = false;

    public static function getSubscribedEvents(): array
    {
        // Try: go to the definition of 'onResponse' or 'onConfigSave', or
        // hover it. Rename onResponse() and this string follows. Misspell
        // a method name and it is reported, since the class has no such
        // method.
        $events[KernelEvents::RESPONSE][] = ['onResponse', 10];
        $events[ConfigEvents::SAVE][] = ['onConfigSave'];
        return $events;
    }

    public function onResponse(ResponseEvent $event): void
    {
        $event->getResponse()->headers->set('X-Acme-Greeting', 'hello');
    }

    public function onConfigSave(ConfigCrudEvent $event): void
    {
        if ($event->getConfig()->getName() === 'acme.settings') {
            $this->greetingChanged = true;
        }
    }
}
