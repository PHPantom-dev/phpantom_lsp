<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\DependencyInjection\ParameterBag;

use Symfony\Component\DependencyInjection\Container;

/**
 * @author Nicolas Grekas <p@tchwork.com>
 */
class ContainerBag extends FrozenParameterBag implements ContainerBagInterface
{
    public function __construct(
        private Container $container,
    ) {}

    public function all(): array
    {}

    public function get(string $name): array|bool|string|int|float|\UnitEnum|null
    {}

    public function has(string $name): bool
    {}
}
