<?php

namespace App\Query;

use App\Message\CountWelcomeEmails;
use Symfony\Component\Messenger\HandleTrait;
use Symfony\Component\Messenger\MessageBusInterface;

final class QueryBus
{
    use HandleTrait;

    public function __construct(MessageBusInterface $queryBus)
    {
        $this->messageBus = $queryBus;
    }

    public function countWelcomeEmails(): int
    {
        // Try: hover $count. HandleTrait::handle() returns whatever the one
        // handler of the message returns, and
        // src/MessageHandler/CountWelcomeEmailsHandler.php returns an int.
        $count = $this->handle(new CountWelcomeEmails());

        return $count;
    }
}
