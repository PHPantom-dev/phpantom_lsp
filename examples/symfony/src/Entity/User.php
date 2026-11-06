<?php

namespace App\Entity;

use Symfony\Component\Serializer\Attribute\Groups;

final class User
{
    public function __construct(
        private string $email,
        private string $displayName,
    ) {}

    #[Groups(['user:read'])]
    public function getEmail(): string
    {
        return $this->email;
    }

    public function setEmail(string $email): void
    {
        $this->email = $email;
    }

    #[Groups(['user:read', 'user:list'])]
    public function getDisplayName(): string
    {
        return $this->displayName;
    }

    public function setDisplayName(string $displayName): void
    {
        $this->displayName = $displayName;
    }
}
