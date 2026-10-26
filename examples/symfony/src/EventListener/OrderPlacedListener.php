<?php

namespace App\EventListener;

use App\Event\OrderPlaced;
use Symfony\Component\EventDispatcher\Attribute\AsEventListener;

// Try: go-to-definition on 'onOrderPlaced' lands on the method below, and a
// method that does not exist is reported.
#[AsEventListener(event: 'app.order.placed', method: 'onOrderPlaced')]
final class OrderPlacedListener
{
    public function onOrderPlaced(OrderPlaced $event): void
    {
        $event->userId;
    }
}
