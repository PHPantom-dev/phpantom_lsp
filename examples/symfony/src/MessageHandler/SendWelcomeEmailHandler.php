<?php

namespace App\MessageHandler;

use App\Message\SendWelcomeEmail;
use App\Service\WelcomeMailer;
use Symfony\Component\Messenger\Attribute\AsMessageHandler;

#[AsMessageHandler(bus: 'command.bus')]
final readonly class SendWelcomeEmailHandler
{
    public function __construct(private WelcomeMailer $mailer) {}

    public function __invoke(SendWelcomeEmail $message): void
    {
        $this->mailer->send($message->userId);
    }
}
