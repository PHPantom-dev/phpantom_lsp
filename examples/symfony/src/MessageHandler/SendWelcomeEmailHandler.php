<?php

namespace App\MessageHandler;

use App\Message\SendWelcomeEmail;
use App\Service\WelcomeMailer;
use Symfony\Component\Messenger\Attribute\AsMessageHandler;

// Try: go-to-definition on the bus name, declared in
// config/packages/messenger.yaml. The CodeLens above __invoke() opens the
// message this handler receives, and the one above
// src/Message/SendWelcomeEmail.php comes back here.
#[AsMessageHandler(bus: 'command.bus')]
final readonly class SendWelcomeEmailHandler
{
    public function __construct(private WelcomeMailer $mailer) {}

    public function __invoke(SendWelcomeEmail $message): void
    {
        $this->mailer->send($message->userId);
    }
}
