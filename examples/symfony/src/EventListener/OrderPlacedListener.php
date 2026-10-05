<?php

namespace App\EventListener;

use App\Event\OrderPlaced;
use Symfony\Component\EventDispatcher\Attribute\AsEventListener;

#[AsEventListener(event: 'app.order.placed', method: 'onOrderPlaced')]
final class OrderPlacedListener
{
    public function onOrderPlaced(OrderPlaced $event): void
    {
        $event->userId;
    }
}
