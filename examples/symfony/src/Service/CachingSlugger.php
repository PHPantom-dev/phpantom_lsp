<?php

namespace App\Service;

/**
 * config/services.yaml has it decorate app.slugger, so the container hands
 * it out in that service's place.
 */
final class CachingSlugger extends Slugger
{
    /** @var array<string, string> */
    private array $slugs = [];

    public function __construct(
        private readonly Slugger $inner,
    ) {}

    public function slug(string $text): string
    {
        return $this->slugs[$text] ??= $this->inner->slug($text);
    }

    public function cachedCount(): int
    {
        return count($this->slugs);
    }
}
