<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Messenger;

use Symfony\Component\Messenger\Stamp\StampInterface;

/**
 * A message wrapped in an envelope with stamps (configurations, markers, ...).
 *
 * @author Maxime Steinhausser <maxime.steinhausser@gmail.com>
 */
final class Envelope
{
    /**
     * @var array<class-string<StampInterface>, list<StampInterface>>
     */
    private array $stamps = [];

    /**
     * @param object|Envelope  $message
     * @param StampInterface[] $stamps
     */
    public function __construct(
        private object $message,
        array $stamps = [],
    ) {}

    /**
     * Makes sure the message is in an Envelope and adds the given stamps.
     *
     * @param StampInterface[] $stamps
     */
    public static function wrap(object $message, array $stamps = []): self
    {}

    /**
     * Adds one or more stamps.
     */
    public function with(StampInterface ...$stamps): static
    {}

    /**
     * Removes all stamps of the given class.
     */
    public function withoutAll(string $stampFqcn): static
    {}

    /**
     * Removes all stamps that implement the given type.
     */
    public function withoutStampsOfType(string $type): self
    {}

    /**
     * @template TStamp of StampInterface
     *
     * @param class-string<TStamp> $stampFqcn
     *
     * @return TStamp|null
     */
    public function last(string $stampFqcn): ?StampInterface
    {}

    /**
     * @template TStamp of StampInterface
     *
     * @param class-string<TStamp>|null $stampFqcn
     *
     * @return StampInterface[]|StampInterface[][] The stamps for the specified FQCN, or all stamps by their class name
     *
     * @psalm-return ($stampFqcn is null ? array<class-string<StampInterface>, list<StampInterface>> : list<TStamp>)
     */
    public function all(?string $stampFqcn = null): array
    {}

    public function getMessage(): object
    {}

    public function __wakeup(): void
    {}
}
