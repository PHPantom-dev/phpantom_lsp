<?php

declare(strict_types=1);

namespace Doctrine\ORM\Mapping\Driver;

use Doctrine\ORM\Events;
use Doctrine\ORM\Mapping;
use Doctrine\ORM\Mapping\Builder\EntityListenerBuilder;
use Doctrine\ORM\Mapping\ClassMetadata;
use Doctrine\ORM\Mapping\MappingException;
use Doctrine\Persistence\Mapping\ClassMetadata as PersistenceClassMetadata;
use Doctrine\Persistence\Mapping\Driver\ClassLocator;
use Doctrine\Persistence\Mapping\Driver\ColocatedMappingDriver;
use Doctrine\Persistence\Mapping\Driver\MappingDriver;
use InvalidArgumentException;
use ReflectionClass;
use ReflectionMethod;

use function assert;
use function class_exists;
use function constant;
use function defined;
use function sprintf;

class AttributeDriver implements MappingDriver
{
    use ColocatedMappingDriver;
    use ReflectionBasedDriver;

    private const ENTITY_ATTRIBUTE_CLASSES = [
        Mapping\Entity::class => 1,
        Mapping\MappedSuperclass::class => 2,
    ];

    private readonly AttributeReader $reader;

    /**
     * @param string[]|ClassLocator $paths                     a ClassLocator, or an array of directories.
     * @param true                  $reportFieldsWhereDeclared no-op, to be removed in 4.0
     */
    public function __construct(array|ClassLocator $paths, bool $reportFieldsWhereDeclared = true)
    {}

    public function isTransient(string $className): bool
    {}

    /**
     * {@inheritDoc}
     *
     * @param class-string<T>  $className
     * @param ClassMetadata<T> $metadata
     *
     * @template T of object
     */
    public function loadMetadataForClass(string $className, PersistenceClassMetadata $metadata): void
    {}

    /**
     * Attempts to resolve the fetch mode.
     *
     * @param class-string $className The class name.
     * @param string       $fetchMode The fetch mode.
     *
     * @return ClassMetadata::FETCH_* The fetch mode as defined in ClassMetadata.
     *
     * @throws MappingException If the fetch mode is not valid.
     */
    private function getFetchMode(string $className, string $fetchMode): int
    {}

    /**
     * Attempts to resolve the generated mode.
     *
     * @throws MappingException If the fetch mode is not valid.
     */
    private function getGeneratedMode(string $generatedMode): int
    {}

    /**
     * Parses the given method.
     *
     * @return list<array{string, string}>
     * @phpstan-return list<array{string, (Events::*)}>
     */
    private function getMethodCallbacks(ReflectionMethod $method): array
    {}

    /**
     * Parse the given JoinColumn as array
     *
     * @return mixed[]
     * @phpstan-return array{
     *                   name: string|null,
     *                   unique: bool,
     *                   nullable: bool,
     *                   onDelete: mixed,
     *                   columnDefinition: string|null,
     *                   referencedColumnName: string,
     *                   foreignKeyName: string|null,
     *                   options?: array<string, mixed>
     *               }
     */
    private function joinColumnToArray(Mapping\JoinColumn|Mapping\InverseJoinColumn $joinColumn): array
    {}

    /**
     * Parse the given Column as array
     *
     * @return mixed[]
     * @phpstan-return array{
     *                   fieldName: string,
     *                   type: mixed,
     *                   scale: int,
     *                   length: int,
     *                   unique: bool,
     *                   nullable: bool,
     *                   index: bool,
     *                   precision: int,
     *                   enumType?: class-string,
     *                   options?: mixed[],
     *                   columnName?: string,
     *                   columnDefinition?: string
     *               }
     */
    private function columnToArray(string $fieldName, Mapping\Column $column): array
    {}
}
