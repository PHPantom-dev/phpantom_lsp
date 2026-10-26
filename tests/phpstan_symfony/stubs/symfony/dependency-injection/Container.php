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

use Symfony\Component\DependencyInjection\Argument\RewindableGenerator;
use Symfony\Component\DependencyInjection\Argument\ServiceLocator as ArgumentServiceLocator;
use Symfony\Component\DependencyInjection\Exception\EnvNotFoundException;
use Symfony\Component\DependencyInjection\Exception\InvalidArgumentException;
use Symfony\Component\DependencyInjection\Exception\ParameterCircularReferenceException;
use Symfony\Component\DependencyInjection\Exception\ParameterNotFoundException;
use Symfony\Component\DependencyInjection\Exception\RuntimeException;
use Symfony\Component\DependencyInjection\Exception\ServiceCircularReferenceException;
use Symfony\Component\DependencyInjection\Exception\ServiceNotFoundException;
use Symfony\Component\DependencyInjection\ParameterBag\EnvPlaceholderParameterBag;
use Symfony\Component\DependencyInjection\ParameterBag\FrozenParameterBag;
use Symfony\Component\DependencyInjection\ParameterBag\ParameterBag;
use Symfony\Component\DependencyInjection\ParameterBag\ParameterBagInterface;
use Symfony\Contracts\Service\ResetInterface;

// Help opcache.preload discover always-needed symbols
class_exists(RewindableGenerator::class);
class_exists(ArgumentServiceLocator::class);

/**
 * Container is a dependency injection container.
 *
 * It gives access to object instances (services).
 * Services and parameters are simple key/pair stores.
 * The container can have five possible behaviors when a service
 * does not exist (or is not initialized for the last case):
 *
 *  * EXCEPTION_ON_INVALID_REFERENCE: Throws an exception at compilation time (the default)
 *  * NULL_ON_INVALID_REFERENCE:      Returns null
 *  * IGNORE_ON_INVALID_REFERENCE:    Ignores the wrapping command asking for the reference
 *                                    (for instance, ignore a setter if the service does not exist)
 *  * IGNORE_ON_UNINITIALIZED_REFERENCE: Ignores/returns null for uninitialized services or invalid references
 *  * RUNTIME_EXCEPTION_ON_INVALID_REFERENCE: Throws an exception at runtime
 *
 * @author Fabien Potencier <fabien@symfony.com>
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 */
class Container implements ContainerInterface, ResetInterface
{
    protected ParameterBagInterface $parameterBag;
    protected array $services = [];
    protected array $privates = [];
    protected array $fileMap = [];
    protected array $methodMap = [];
    protected array $factories = [];
    protected array $aliases = [];
    protected array $loading = [];
    protected array $resolving = [];
    protected array $syntheticIds = [];

    private array $envCache = [];
    private bool $compiled = false;
    private \Closure $getEnv;

    private static \Closure $make;

    public function __construct(?ParameterBagInterface $parameterBag = null)
    {}

    /**
     * Compiles the container.
     *
     * This method does two things:
     *
     *  * Parameter values are resolved;
     *  * The parameter bag is frozen.
     */
    public function compile(): void
    {}

    /**
     * Returns true if the container is compiled.
     */
    public function isCompiled(): bool
    {}

    /**
     * Gets the service container parameter bag.
     */
    public function getParameterBag(): ParameterBagInterface
    {}

    /**
     * Gets a parameter.
     *
     * @throws ParameterNotFoundException if the parameter is not defined
     */
    public function getParameter(string $name): array|bool|string|int|float|\UnitEnum|null
    {}

    public function hasParameter(string $name): bool
    {}

    public function setParameter(string $name, array|bool|string|int|float|\UnitEnum|null $value): void
    {}

    /**
     * Sets a service.
     *
     * Setting a synthetic service to null resets it: has() returns false and get()
     * behaves in the same way as if the service was never created.
     */
    public function set(string $id, ?object $service): void
    {}

    public function has(string $id): bool
    {}

    /**
     * Gets a service.
     *
     * @throws ServiceCircularReferenceException When a circular reference is detected
     * @throws ServiceNotFoundException          When the service is not defined
     *
     * @see Reference
     */
    public function get(string $id, int $invalidBehavior = self::EXCEPTION_ON_INVALID_REFERENCE): ?object
    {}

    /**
     * Creates a service.
     *
     * As a separate method to allow "get()" to use the really fast `??` operator.
     */
    private static function make(self $container, string $id, int $invalidBehavior): ?object
    {}

    /**
     * Returns true if the given service has actually been initialized.
     */
    public function initialized(string $id): bool
    {}

    public function reset(): void
    {}

    /**
     * @internal
     */
    public function resetEnvCache(): void
    {}

    /**
     * Gets all service ids.
     *
     * @return string[]
     */
    public function getServiceIds(): array
    {}

    /**
     * Gets service ids that existed at compile time.
     */
    public function getRemovedIds(): array
    {}

    /**
     * Camelizes a string.
     */
    public static function camelize(string $id): string
    {}

    /**
     * A string to underscore.
     */
    public static function underscore(string $id): string
    {}

    /**
     * Creates a service by requiring its factory file.
     */
    protected function load(string $file): mixed
    {}

    /**
     * Fetches a variable from the environment.
     *
     * @throws EnvNotFoundException When the environment variable is not found and has no default value
     */
    protected function getEnv(string $name): mixed
    {}

    /**
     * @internal
     */
    final protected function getService(string|false $registry, string $id, ?string $method, string|bool $load): mixed
    {}

    private function __clone()
    {}
}
