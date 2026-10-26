<?php

namespace App\MessageHandler;

use App\Message\CountWelcomeEmails;
use Symfony\Component\Messenger\Attribute\AsMessageHandler;

#[AsMessageHandler(bus: 'query.bus')]
final readonly class CountWelcomeEmailsHandler
{
    public function __invoke(CountWelcomeEmails $query): int
    {
        return 0;
    }
}
