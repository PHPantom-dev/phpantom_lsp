<?php

namespace App\Event;

final readonly class OrderPlaced
{
    public function __construct(public string $userId) {}
}
