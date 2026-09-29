<?php

namespace App\Entity;

final class User
{
    public function __construct(
        private string $email,
        private string $displayName,
    ) {}

    public function email(): string
    {
        return $this->email;
    }

    public function displayName(): string
    {
        return $this->displayName;
    }
}
