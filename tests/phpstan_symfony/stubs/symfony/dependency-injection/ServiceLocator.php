<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\DependencyInjection;

use Psr\Container\ContainerExceptionInterface;
use Psr\Container\NotFoundExceptionInterface;
use Symfony\Component\DependencyInjection\Exception\RuntimeException;
use Symfony\Component\DependencyInjection\Exception\ServiceCircularReferenceException;
use Symfony\Component\DependencyInjection\Exception\ServiceNotFoundException;
use Symfony\Contracts\Service\ServiceCollectionInterface;
use Symfony\Contracts\Service\ServiceLocatorTrait;
use Symfony\Contracts\Service\ServiceSubscriberInterface;

/**
 * @author Robin Chalas <robin.chalas@gmail.com>
 * @author Nicolas Grekas <p@tchwork.com>
 *
 * @template-covariant T of mixed
 *
 * @implements ServiceCollectionInterface<T>
 */
class ServiceLocator implements ServiceCollectionInterface
{
    use ServiceLocatorTrait {
        get as private doGet;
    }

    private ?string $externalId = null;
    private ?Container $container = null;

    public function get(string $id): mixed
    {}

    public function __invoke(string $id): mixed
    {}

    /**
     * @internal
     */
    public function withContext(string $externalId, Container $container): static
    {}

    public function count(): int
    {}

    public function getIterator(): \Traversable
    {}

    private function createNotFoundException(string $id): NotFoundExceptionInterface
    {}

    private function createCircularReferenceException(string $id, array $path): ContainerExceptionInterface
    {}

    private function formatAlternatives(?array $alternatives = null, string $separator = 'and'): string
    {}
}
