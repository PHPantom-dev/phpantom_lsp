<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\HttpKernel;

use Symfony\Component\Config\Builder\ConfigBuilderGenerator;
use Symfony\Component\Config\ConfigCache;
use Symfony\Component\Config\Loader\DelegatingLoader;
use Symfony\Component\Config\Loader\LoaderResolver;
use Symfony\Component\DependencyInjection\Compiler\CompilerPassInterface;
use Symfony\Component\DependencyInjection\Compiler\PassConfig;
use Symfony\Component\DependencyInjection\Compiler\RemoveBuildParametersPass;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\DependencyInjection\ContainerInterface;
use Symfony\Component\DependencyInjection\Dumper\PhpDumper;
use Symfony\Component\DependencyInjection\Dumper\Preloader;
use Symfony\Component\DependencyInjection\Extension\ExtensionInterface;
use Symfony\Component\DependencyInjection\Loader\ClosureLoader;
use Symfony\Component\DependencyInjection\Loader\DirectoryLoader;
use Symfony\Component\DependencyInjection\Loader\GlobFileLoader;
use Symfony\Component\DependencyInjection\Loader\IniFileLoader;
use Symfony\Component\DependencyInjection\Loader\PhpFileLoader;
use Symfony\Component\DependencyInjection\Loader\XmlFileLoader;
use Symfony\Component\DependencyInjection\Loader\YamlFileLoader;
use Symfony\Component\ErrorHandler\DebugClassLoader;
use Symfony\Component\Filesystem\Filesystem;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\HttpKernel\Bundle\BundleInterface;
use Symfony\Component\HttpKernel\CacheWarmer\WarmableInterface;
use Symfony\Component\HttpKernel\Config\FileLocator;
use Symfony\Component\HttpKernel\DependencyInjection\MergeExtensionConfigurationPass;

// Help opcache.preload discover always-needed symbols
class_exists(ConfigCache::class);

/**
 * The Kernel is the heart of the Symfony system.
 *
 * It manages an environment made of bundles.
 *
 * Environment names must always start with a letter and
 * they must only contain letters and numbers.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
abstract class Kernel implements KernelInterface, RebootableInterface, TerminableInterface
{
    /**
     * @var array<string, BundleInterface>
     */
    protected array $bundles = [];

    protected ?ContainerInterface $container = null;
    protected bool $booted = false;
    protected ?float $startTime = null;

    private string $projectDir;
    private ?string $warmupDir = null;
    private int $requestStackSize = 0;
    private bool $resetServices = false;
    private bool $handlingHttpCache = false;

    /**
     * @var array<string, bool>
     */
    private static array $freshCache = [];

    public const VERSION = '7.4.21';
    public const VERSION_ID = 70421;
    public const MAJOR_VERSION = 7;
    public const MINOR_VERSION = 4;
    public const RELEASE_VERSION = 21;
    public const EXTRA_VERSION = '';

    public const END_OF_MAINTENANCE = '11/2028';
    public const END_OF_LIFE = '11/2029';

    public function __construct(
        protected string $environment,
        protected bool $debug,
    ) {}

    public function __clone()
    {}

    public function boot(): void
    {}

    public function reboot(?string $warmupDir): void
    {}

    public function terminate(Request $request, Response $response): void
    {}

    public function shutdown(): void
    {}

    public function handle(Request $request, int $type = HttpKernelInterface::MAIN_REQUEST, bool $catch = true): Response
    {}

    /**
     * Gets an HTTP kernel from the container.
     */
    protected function getHttpKernel(): HttpKernelInterface
    {}

    public function getBundles(): array
    {}

    public function getBundle(string $name): BundleInterface
    {}

    public function locateResource(string $name): string
    {}

    public function getEnvironment(): string
    {}

    public function isDebug(): bool
    {}

    /**
     * Gets the application root dir (path of the project's composer file).
     */
    public function getProjectDir(): string
    {}

    public function getContainer(): ContainerInterface
    {}

    /**
     * @internal
     *
     * @deprecated since Symfony 7.1, to be removed in 8.0
     */
    public function setAnnotatedClassCache(array $annotatedClasses): void
    {}

    public function getStartTime(): float
    {}

    public function getCacheDir(): string
    {}

    public function getBuildDir(): string
    {}

    public function getShareDir(): ?string
    {}

    public function getLogDir(): string
    {}

    public function getCharset(): string
    {}

    /**
     * Gets the patterns defining the classes to parse and cache for annotations.
     *
     * @return string[]
     *
     * @deprecated since Symfony 7.1, to be removed in 8.0
     */
    public function getAnnotatedClassesToCompile(): array
    {}

    /**
     * Initializes bundles.
     *
     * @throws \LogicException if two bundles share a common name
     */
    protected function initializeBundles(): void
    {}

    /**
     * The extension point similar to the Bundle::build() method.
     *
     * Use this method to register compiler passes and manipulate the container during the building process.
     */
    protected function build(ContainerBuilder $container): void
    {}

    /**
     * Gets the container class.
     *
     * @throws \InvalidArgumentException If the generated classname is invalid
     */
    protected function getContainerClass(): string
    {}

    /**
     * Gets the container's base class.
     *
     * All names except Container must be fully qualified.
     */
    protected function getContainerBaseClass(): string
    {}

    /**
     * Initializes the service container.
     *
     * The built version of the service container is used when fresh, otherwise the
     * container is built.
     */
    protected function initializeContainer(): void
    {}

    /**
     * Returns the kernel parameters.
     *
     * @return array<string, array|bool|string|int|float|\UnitEnum|null>
     */
    protected function getKernelParameters(): array
    {}

    /**
     * Builds the service container.
     *
     * @throws \RuntimeException
     */
    protected function buildContainer(): ContainerBuilder
    {}

    /**
     * Prepares the ContainerBuilder before it is compiled.
     */
    protected function prepareContainer(ContainerBuilder $container): void
    {}

    /**
     * Gets a new ContainerBuilder instance used to build the service container.
     */
    protected function getContainerBuilder(): ContainerBuilder
    {}

    /**
     * Dumps the service container to PHP code in the cache.
     *
     * @param string $class     The name of the class to generate
     * @param string $baseClass The name of the container's base class
     */
    protected function dumpContainer(ConfigCache $cache, ContainerBuilder $container, string $class, string $baseClass): void
    {}

    /**
     * Returns a loader for the container.
     */
    protected function getContainerLoader(ContainerInterface $container): DelegatingLoader
    {}

    private function preBoot(): ContainerInterface
    {}

    public function __serialize(): array
    {}

    public function __unserialize(array $data): void
    {}

    /**
     * @deprecated since Symfony 7.4, will be replaced by `__serialize()` in 8.0
     */
    public function __sleep(): array
    {}

    /**
     * @deprecated since Symfony 7.4, will be replaced by `__unserialize()` in 8.0
     */
    public function __wakeup(): void
    {}
}
