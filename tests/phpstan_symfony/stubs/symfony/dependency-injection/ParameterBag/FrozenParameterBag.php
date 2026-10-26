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

use Symfony\Component\DependencyInjection\Exception\LogicException;

/**
 * Holds read-only parameters.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class FrozenParameterBag extends ParameterBag
{
    /**
     * For performance reasons, the constructor assumes that
     * all keys are already lowercased.
     *
     * This is always the case when used internally.
     */
    public function __construct(
        array $parameters = [],
        protected array $deprecatedParameters = [],
        protected array $nonEmptyParameters = [],
    ) {}

    public function clear(): never
    {}

    public function add(array $parameters): never
    {}

    public function set(string $name, array|bool|string|int|float|\UnitEnum|null $value): never
    {}

    public function deprecate(string $name, string $package, string $version, string $message = 'The parameter "%s" is deprecated.'): never
    {}

    public function cannotBeEmpty(string $name, string $message = 'A non-empty parameter "%s" is required.'): never
    {}

    public function remove(string $name): never
    {}
}
