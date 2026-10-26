<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\PropertyAccess;

use Psr\Cache\CacheItemPoolInterface;
use Psr\Log\LoggerInterface;
use Psr\Log\NullLogger;
use Symfony\Component\Cache\Adapter\AdapterInterface;
use Symfony\Component\Cache\Adapter\ApcuAdapter;
use Symfony\Component\Cache\Adapter\NullAdapter;
use Symfony\Component\PropertyAccess\Exception\AccessException;
use Symfony\Component\PropertyAccess\Exception\InvalidTypeException;
use Symfony\Component\PropertyAccess\Exception\NoSuchIndexException;
use Symfony\Component\PropertyAccess\Exception\NoSuchPropertyException;
use Symfony\Component\PropertyAccess\Exception\UnexpectedTypeException;
use Symfony\Component\PropertyAccess\Exception\UninitializedPropertyException;
use Symfony\Component\PropertyInfo\Extractor\ReflectionExtractor;
use Symfony\Component\PropertyInfo\PropertyReadInfo;
use Symfony\Component\PropertyInfo\PropertyReadInfoExtractorInterface;
use Symfony\Component\PropertyInfo\PropertyWriteInfo;
use Symfony\Component\PropertyInfo\PropertyWriteInfoExtractorInterface;

/**
 * Default implementation of {@link PropertyAccessorInterface}.
 *
 * @author Bernhard Schussek <bschussek@gmail.com>
 * @author Kévin Dunglas <dunglas@gmail.com>
 * @author Nicolas Grekas <p@tchwork.com>
 */
class PropertyAccessor implements PropertyAccessorInterface
{
    /** @var int Allow none of the magic methods */
    public const DISALLOW_MAGIC_METHODS = ReflectionExtractor::DISALLOW_MAGIC_METHODS;
    /** @var int Allow magic __get methods */
    public const MAGIC_GET = ReflectionExtractor::ALLOW_MAGIC_GET;
    /** @var int Allow magic __set methods */
    public const MAGIC_SET = ReflectionExtractor::ALLOW_MAGIC_SET;
    /** @var int Allow magic __call methods */
    public const MAGIC_CALL = ReflectionExtractor::ALLOW_MAGIC_CALL;

    public const DO_NOT_THROW = 0;
    public const THROW_ON_INVALID_INDEX = 1;
    public const THROW_ON_INVALID_PROPERTY_PATH = 2;

    private const VALUE = 0;
    private const REF = 1;
    private const IS_REF_CHAINED = 2;
    private const CACHE_PREFIX_READ = 'r';
    private const CACHE_PREFIX_WRITE = 'w';
    private const CACHE_PREFIX_PROPERTY_PATH = 'p';
    private const RESULT_PROTO = [self::VALUE => null];

    private bool $ignoreInvalidIndices;
    private bool $ignoreInvalidProperty;
    private ?CacheItemPoolInterface $cacheItemPool;
    private array $propertyPathCache = [];
    private PropertyReadInfoExtractorInterface $readInfoExtractor;
    private PropertyWriteInfoExtractorInterface $writeInfoExtractor;
    private array $readPropertyCache = [];
    private array $writePropertyCache = [];

    /**
     * Should not be used by application code. Use
     * {@link PropertyAccess::createPropertyAccessor()} instead.
     *
     * @param int $magicMethodsFlags A bitwise combination of the MAGIC_* constants
     *                               to specify the allowed magic methods (__get, __set, __call)
     *                               or self::DISALLOW_MAGIC_METHODS for none
     * @param int $throw             A bitwise combination of the THROW_* constants
     *                               to specify when exceptions should be thrown
     */
    public function __construct(
        private int $magicMethodsFlags = self::MAGIC_GET | self::MAGIC_SET,
        int $throw = self::THROW_ON_INVALID_PROPERTY_PATH,
        ?CacheItemPoolInterface $cacheItemPool = null,
        ?PropertyReadInfoExtractorInterface $readInfoExtractor = null,
        ?PropertyWriteInfoExtractorInterface $writeInfoExtractor = null,
    ) {}

    public function getValue(object|array $objectOrArray, string|PropertyPathInterface $propertyPath): mixed
    {}

    public function setValue(object|array &$objectOrArray, string|PropertyPathInterface $propertyPath, mixed $value): void
    {}

    private static function throwInvalidArgumentException(string $message, array $trace, int $i, string $propertyPath, ?\Throwable $previous = null): void
    {}

    public function isReadable(object|array $objectOrArray, string|PropertyPathInterface $propertyPath): bool
    {}

    public function isWritable(object|array $objectOrArray, string|PropertyPathInterface $propertyPath): bool
    {}

    /**
     * Reads the path from an object up to a given path index.
     *
     * @throws UnexpectedTypeException if a value within the path is neither object nor array
     * @throws NoSuchIndexException    If a non-existing index is accessed
     */
    private function readPropertiesUntil(array $zval, PropertyPathInterface $propertyPath, int $lastIndex, bool $ignoreInvalidIndices = true): array
    {}

    /**
     * Reads a key from an array-like structure.
     *
     * @throws NoSuchIndexException If the array does not implement \ArrayAccess or it is not an array
     */
    private function readIndex(array $zval, string|int $index): array
    {}

    /**
     * Reads the value of a property from an object.
     *
     * @throws NoSuchPropertyException If $ignoreInvalidProperty is false and the property does not exist or is not public
     */
    private function readProperty(array $zval, string $property, bool $ignoreInvalidProperty = false, bool $isNullSafe = false): array
    {}

    /**
     * Guesses how to read the property value.
     */
    private function getReadInfo(string $class, string $property): ?PropertyReadInfo
    {}

    /**
     * Sets the value of an index in a given array-accessible value.
     *
     * @throws NoSuchIndexException If the array does not implement \ArrayAccess or it is not an array
     */
    private function writeIndex(array $zval, string|int $index, mixed $value): void
    {}

    /**
     * Sets the value of a property in the given object.
     *
     * @throws NoSuchPropertyException if the property does not exist or is not public
     */
    private function writeProperty(array $zval, string $property, mixed $value, bool $recursive = false): void
    {}

    /**
     * Adjusts a collection-valued property by calling add*() and remove*() methods.
     */
    private function writeCollection(array $zval, string $property, iterable $collection, PropertyWriteInfo $addMethod, PropertyWriteInfo $removeMethod): void
    {}

    private function getWriteInfo(string $class, string $property, mixed $value): PropertyWriteInfo
    {}

    /**
     * Returns whether a property is writable in the given object.
     */
    private function isPropertyWritable(object $object, string $property): bool
    {}

    /**
     * Gets a PropertyPath instance and caches it.
     */
    private function getPropertyPath(string|PropertyPath $propertyPath): PropertyPath
    {}

    /**
     * Creates the APCu adapter if applicable.
     *
     * @throws \LogicException When the Cache Component isn't available
     */
    public static function createCache(string $namespace, int $defaultLifetime, string $version, ?LoggerInterface $logger = null): AdapterInterface
    {}
}
