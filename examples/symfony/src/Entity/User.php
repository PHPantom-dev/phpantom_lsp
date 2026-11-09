<?php

namespace App\Entity;

use Symfony\Component\Serializer\Attribute\Groups;

final class User
{
    private ?Team $team = null;

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

    public function getTeam(): ?Team
    {
        return $this->team;
    }

    public function setTeam(?Team $team): void
    {
        $this->team = $team;
    }
}
