<?php

namespace App\Support;

class BakeryService
{
    private string $temperature = 'medium';

    public function bake(string $item): string
    {
        return "a fresh {$item}";
    }

    public function heatedTo(string $temperature): static
    {
        $this->temperature = $temperature;

        return $this;
    }

    public function temperature(): string
    {
        return $this->temperature;
    }
}
