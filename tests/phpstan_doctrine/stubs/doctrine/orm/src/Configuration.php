<?php

declare(strict_types=1);

namespace Doctrine\ORM;

use Doctrine\DBAL\Platforms\AbstractPlatform;
use Doctrine\DBAL\Schema\Schema;
use Doctrine\Deprecations\Deprecation;
use Doctrine\ORM\Cache\CacheConfiguration;
use Doctrine\ORM\Exception\InvalidEntityRepository;
use Doctrine\ORM\Internal\Hydration\AbstractHydrator;
use Doctrine\ORM\Mapping\ClassMetadata;
use Doctrine\ORM\Mapping\ClassMetadataFactory;
use Doctrine\ORM\Mapping\DefaultEntityListenerResolver;
use Doctrine\ORM\Mapping\DefaultNamingStrategy;
use Doctrine\ORM\Mapping\DefaultQuoteStrategy;
use Doctrine\ORM\Mapping\EntityListenerResolver;
use Doctrine\ORM\Mapping\NamingStrategy;
use Doctrine\ORM\Mapping\QuoteStrategy;
use Doctrine\ORM\Mapping\TypedFieldMapper;
use Doctrine\ORM\Proxy\ProxyFactory;
use Doctrine\ORM\Query\AST\Functions\FunctionNode;
use Doctrine\ORM\Query\Filter\SQLFilter;
use Doctrine\ORM\Repository\DefaultRepositoryFactory;
use Doctrine\ORM\Repository\RepositoryFactory;
use Doctrine\Persistence\Mapping\Driver\MappingDriver;
use LogicException;
use Psr\Cache\CacheItemPoolInterface;

use function class_exists;
use function is_a;
use function method_exists;
use function strtolower;

use const PHP_VERSION_ID;

/**
 * Configuration container for all configuration options of Doctrine.
 * It combines all configuration options from DBAL & ORM.
 *
 * Internal note: When adding a new configuration option just write a getter/setter pair.
 */
class Configuration extends \Doctrine\DBAL\Configuration
{
    /** @var mixed[] */
    protected array $attributes = [];

    /** @phpstan-var array<class-string<AbstractPlatform>, ClassMetadata::GENERATOR_TYPE_*> */
    private $identityGenerationPreferences = [];

    /** @phpstan-param array<class-string<AbstractPlatform>, ClassMetadata::GENERATOR_TYPE_*> $value */
    public function setIdentityGenerationPreferences(array $value): void
    {}

    /** @phpstan-return array<class-string<AbstractPlatform>, ClassMetadata::GENERATOR_TYPE_*> $value */
    public function getIdentityGenerationPreferences(): array
    {}

    /**
     * Sets the directory where Doctrine generates any necessary proxy class files.
     */
    public function setProxyDir(string $dir): void
    {}

    /**
     * Gets the directory where Doctrine generates any necessary proxy class files.
     */
    public function getProxyDir(): string|null
    {}

    /**
     * Gets the strategy for automatically generating proxy classes.
     *
     * @return ProxyFactory::AUTOGENERATE_*
     */
    public function getAutoGenerateProxyClasses(): int
    {}

    /**
     * Sets the strategy for automatically generating proxy classes.
     *
     * @param bool|ProxyFactory::AUTOGENERATE_* $autoGenerate True is converted to AUTOGENERATE_ALWAYS, false to AUTOGENERATE_NEVER.
     */
    public function setAutoGenerateProxyClasses(bool|int $autoGenerate): void
    {}

    /**
     * Gets the namespace where proxy classes reside.
     */
    public function getProxyNamespace(): string|null
    {}

    /**
     * Sets the namespace where proxy classes reside.
     */
    public function setProxyNamespace(string $ns): void
    {}

    public function getUseDbalEditorApi(): bool
    {}

    /**
     * @internal
     *
     * When releasing this, add an UPGRADE note about MySQL's foreign key name length issue
     */
    public function setUseDbalEditorApi(bool $useDbalEditorApi): void
    {}

    /**
     * Sets the cache driver implementation that is used for metadata caching.
     *
     * @todo Force parameter to be a Closure to ensure lazy evaluation
     *       (as soon as a metadata cache is in effect, the driver never needs to initialize).
     */
    public function setMetadataDriverImpl(MappingDriver $driverImpl): void
    {}

    /**
     * Sets the entity alias map.
     *
     * @phpstan-param array<string, string> $entityNamespaces
     */
    public function setEntityNamespaces(array $entityNamespaces): void
    {}

    /**
     * Retrieves the list of registered entity namespace aliases.
     *
     * @phpstan-return array<string, string>
     */
    public function getEntityNamespaces(): array
    {}

    /**
     * Gets the cache driver implementation that is used for the mapping metadata.
     */
    public function getMetadataDriverImpl(): MappingDriver|null
    {}

    /**
     * Gets the cache driver implementation that is used for the query cache (SQL cache).
     */
    public function getQueryCache(): CacheItemPoolInterface|null
    {}

    /**
     * Sets the cache driver implementation that is used for the query cache (SQL cache).
     */
    public function setQueryCache(CacheItemPoolInterface $cache): void
    {}

    public function getHydrationCache(): CacheItemPoolInterface|null
    {}

    public function setHydrationCache(CacheItemPoolInterface $cache): void
    {}

    public function getMetadataCache(): CacheItemPoolInterface|null
    {}

    public function setMetadataCache(CacheItemPoolInterface $cache): void
    {}

    /**
     * Registers a custom DQL function that produces a string value.
     * Such a function can then be used in any DQL statement in any place where string
     * functions are allowed.
     *
     * DQL function names are case-insensitive.
     *
     * @param class-string|callable $className Class name or a callable that returns the function.
     * @phpstan-param class-string<FunctionNode>|callable(string):FunctionNode $className
     */
    public function addCustomStringFunction(string $name, string|callable $className): void
    {}

    /**
     * Gets the implementation class name of a registered custom string DQL function.
     *
     * @phpstan-return class-string<FunctionNode>|callable(string):FunctionNode|null
     */
    public function getCustomStringFunction(string $name): string|callable|null
    {}

    /**
     * Sets a map of custom DQL string functions.
     *
     * Keys must be function names and values the FQCN of the implementing class.
     * The function names will be case-insensitive in DQL.
     *
     * Any previously added string functions are discarded.
     *
     * @phpstan-param array<string, class-string<FunctionNode>|callable(string):FunctionNode> $functions The map of custom
     *                                                     DQL string functions.
     */
    public function setCustomStringFunctions(array $functions): void
    {}

    /**
     * Registers a custom DQL function that produces a numeric value.
     * Such a function can then be used in any DQL statement in any place where numeric
     * functions are allowed.
     *
     * DQL function names are case-insensitive.
     *
     * @param class-string|callable $className Class name or a callable that returns the function.
     * @phpstan-param class-string<FunctionNode>|callable(string):FunctionNode $className
     */
    public function addCustomNumericFunction(string $name, string|callable $className): void
    {}

    /**
     * Gets the implementation class name of a registered custom numeric DQL function.
     *
     * @phpstan-return class-string<FunctionNode>|callable(string):FunctionNode|null
     */
    public function getCustomNumericFunction(string $name): string|callable|null
    {}

    /**
     * Sets a map of custom DQL numeric functions.
     *
     * Keys must be function names and values the FQCN of the implementing class.
     * The function names will be case-insensitive in DQL.
     *
     * Any previously added numeric functions are discarded.
     *
     * @param array<string, class-string> $functions The map of custom
     *                                               DQL numeric functions.
     */
    public function setCustomNumericFunctions(array $functions): void
    {}

    /**
     * Registers a custom DQL function that produces a date/time value.
     * Such a function can then be used in any DQL statement in any place where date/time
     * functions are allowed.
     *
     * DQL function names are case-insensitive.
     *
     * @param string|callable $className Class name or a callable that returns the function.
     * @phpstan-param class-string<FunctionNode>|callable(string):FunctionNode $className
     */
    public function addCustomDatetimeFunction(string $name, string|callable $className): void
    {}

    /**
     * Gets the implementation class name of a registered custom date/time DQL function.
     *
     * @return class-string|callable|null
     */
    public function getCustomDatetimeFunction(string $name): string|callable|null
    {}

    /**
     * Sets a map of custom DQL date/time functions.
     *
     * Keys must be function names and values the FQCN of the implementing class.
     * The function names will be case-insensitive in DQL.
     *
     * Any previously added date/time functions are discarded.
     *
     * @param array $functions The map of custom DQL date/time functions.
     * @phpstan-param array<string, class-string<FunctionNode>|callable(string):FunctionNode> $functions
     */
    public function setCustomDatetimeFunctions(array $functions): void
    {}

    /**
     * Sets a TypedFieldMapper for php typed fields to DBAL types auto-completion.
     */
    public function setTypedFieldMapper(TypedFieldMapper|null $typedFieldMapper): void
    {}

    /**
     * Gets a TypedFieldMapper for php typed fields to DBAL types auto-completion.
     */
    public function getTypedFieldMapper(): TypedFieldMapper|null
    {}

    /**
     * Sets the custom hydrator modes in one pass.
     *
     * @param array<string, class-string<AbstractHydrator>> $modes An array of ($modeName => $hydrator).
     */
    public function setCustomHydrationModes(array $modes): void
    {}

    /**
     * Gets the hydrator class for the given hydration mode name.
     *
     * @return class-string<AbstractHydrator>|null
     */
    public function getCustomHydrationMode(string $modeName): string|null
    {}

    /**
     * Adds a custom hydration mode.
     *
     * @param class-string<AbstractHydrator> $hydrator
     */
    public function addCustomHydrationMode(string $modeName, string $hydrator): void
    {}

    /**
     * Sets a class metadata factory.
     *
     * @param class-string $cmfName
     */
    public function setClassMetadataFactoryName(string $cmfName): void
    {}

    /** @return class-string */
    public function getClassMetadataFactoryName(): string
    {}

    /**
     * Adds a filter to the list of possible filters.
     *
     * @param class-string<SQLFilter> $className The class name of the filter.
     */
    public function addFilter(string $name, string $className): void
    {}

    /**
     * Gets the class name for a given filter name.
     *
     * @return class-string<SQLFilter>|null The class name of the filter,
     *                                      or null if it is not defined.
     */
    public function getFilterClassName(string $name): string|null
    {}

    /**
     * Sets default repository class.
     *
     * @param class-string<EntityRepository> $className
     *
     * @throws InvalidEntityRepository If $classname is not an ObjectRepository.
     */
    public function setDefaultRepositoryClassName(string $className): void
    {}

    /**
     * Get default repository class.
     *
     * @return class-string<EntityRepository>
     */
    public function getDefaultRepositoryClassName(): string
    {}

    /**
     * Sets naming strategy.
     */
    public function setNamingStrategy(NamingStrategy $namingStrategy): void
    {}

    /**
     * Gets naming strategy..
     */
    public function getNamingStrategy(): NamingStrategy
    {}

    /**
     * Sets quote strategy.
     */
    public function setQuoteStrategy(QuoteStrategy $quoteStrategy): void
    {}

    /**
     * Gets quote strategy.
     */
    public function getQuoteStrategy(): QuoteStrategy
    {}

    /**
     * Set the entity listener resolver.
     */
    public function setEntityListenerResolver(EntityListenerResolver $resolver): void
    {}

    /**
     * Get the entity listener resolver.
     */
    public function getEntityListenerResolver(): EntityListenerResolver
    {}

    /**
     * Set the entity repository factory.
     */
    public function setRepositoryFactory(RepositoryFactory $repositoryFactory): void
    {}

    /**
     * Get the entity repository factory.
     */
    public function getRepositoryFactory(): RepositoryFactory
    {}

    public function isSecondLevelCacheEnabled(): bool
    {}

    public function setSecondLevelCacheEnabled(bool $flag = true): void
    {}

    public function setSecondLevelCacheConfiguration(CacheConfiguration $cacheConfig): void
    {}

    public function getSecondLevelCacheConfiguration(): CacheConfiguration|null
    {}

    /**
     * Returns query hints, which will be applied to every query in application
     *
     * @phpstan-return array<string, mixed>
     */
    public function getDefaultQueryHints(): array
    {}

    /**
     * Sets array of query hints, which will be applied to every query in application
     *
     * @phpstan-param array<string, mixed> $defaultQueryHints
     */
    public function setDefaultQueryHints(array $defaultQueryHints): void
    {}

    /**
     * Gets the value of a default query hint. If the hint name is not recognized, FALSE is returned.
     *
     * @return mixed The value of the hint or FALSE, if the hint name is not recognized.
     */
    public function getDefaultQueryHint(string $name): mixed
    {}

    /**
     * Sets a default query hint. If the hint name is not recognized, it is silently ignored.
     */
    public function setDefaultQueryHint(string $name, mixed $value): void
    {}

    /**
     * Gets a list of entity class names to be ignored by the SchemaTool
     *
     * @return list<class-string>
     */
    public function getSchemaIgnoreClasses(): array
    {}

    /**
     * Sets a list of entity class names to be ignored by the SchemaTool
     *
     * @param list<class-string> $schemaIgnoreClasses List of entity class names
     */
    public function setSchemaIgnoreClasses(array $schemaIgnoreClasses): void
    {}

    public function isNativeLazyObjectsEnabled(): bool
    {}

    public function enableNativeLazyObjects(bool $nativeLazyObjects): void
    {}

    /**
     * @deprecated lazy ghost objects are always enabled
     *
     * @return true
     */
    public function isLazyGhostObjectEnabled(): bool
    {}

    /** @deprecated lazy ghost objects cannot be disabled */
    public function setLazyGhostObjectEnabled(bool $flag): void
    {}

    /** @deprecated rejecting ID collisions in the identity map cannot be disabled */
    public function setRejectIdCollisionInIdentityMap(bool $flag): void
    {}

    /**
     * @deprecated rejecting ID collisions in the identity map is always enabled
     *
     * @return true
     */
    public function isRejectIdCollisionInIdentityMapEnabled(): bool
    {}

    public function setEagerFetchBatchSize(int $batchSize = 100): void
    {}

    public function getEagerFetchBatchSize(): int
    {}

    public function setDefaultStringTypeSchemaLength(int $length): void
    {}

    public function getDefaultStringTypeSchemaLength(): int
    {}
}
