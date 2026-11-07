<?php

declare(strict_types=1);

namespace App;

final readonly class Book
{
    public function __construct(public string $title) {}

    public function summary(): string
    {
        return "Read {$this->title}";
    }
}
