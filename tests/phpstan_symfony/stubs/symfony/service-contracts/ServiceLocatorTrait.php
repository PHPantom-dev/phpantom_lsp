<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Contracts\Service;

use Psr\Container\ContainerExceptionInterface;
use Psr\Container\NotFoundExceptionInterface;

// Help opcache.preload discover always-needed symbols
class_exists(ContainerExceptionInterface::class);
class_exists(NotFoundExceptionInterface::class);

/**
 * A trait to help implement ServiceProviderInterface.
 *
 * @author Robin Chalas <robin.chalas@gmail.com>
 * @author Nicolas Grekas <p@tchwork.com>
 */
trait ServiceLocatorTrait
{
    private array $loading = [];
    private array $providedTypes;

    /**
     * @param array<string, callable> $factories
     */
    public function __construct(
        private array $factories,
    ) {}

    public function has(string $id): bool
    {}

    public function get(string $id): mixed
    {}

    public function getProvidedServices(): array
    {}

    private function createNotFoundException(string $id): NotFoundExceptionInterface
    {}

    private function createCircularReferenceException(string $id, array $path): ContainerExceptionInterface
    {}
}
