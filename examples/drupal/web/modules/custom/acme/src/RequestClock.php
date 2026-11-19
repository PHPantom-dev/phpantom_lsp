<?php

declare(strict_types=1);

namespace Drupal\acme;

use Drupal\Component\Datetime\TimeInterface;

/**
 * Core's clock, with how long the current request has run.
 *
 * acme.services.yml has it decorate core's datetime.time, so the container
 * hands it out in that service's place.
 */
final class RequestClock implements TimeInterface
{
    public function __construct(
        private readonly TimeInterface $inner,
    ) {}

    public function getRequestTime(): int
    {
        return $this->inner->getRequestTime();
    }

    public function getRequestMicroTime(): float
    {
        return $this->inner->getRequestMicroTime();
    }

    public function getCurrentTime(): int
    {
        return $this->inner->getCurrentTime();
    }

    public function getCurrentMicroTime(): float
    {
        return $this->inner->getCurrentMicroTime();
    }

    public function secondsSinceRequest(): int
    {
        return $this->getCurrentTime() - $this->getRequestTime();
    }
}
