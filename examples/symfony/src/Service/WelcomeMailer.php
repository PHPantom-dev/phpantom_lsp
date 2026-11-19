<?php

namespace App\Service;

final class WelcomeMailer
{
    private string $sender = '';

    public function setSender(string $sender): void
    {
        $this->sender = $sender;
    }

    public function send(string $userId): void
    {
        $userId;
        $this->sender;
    }
}
