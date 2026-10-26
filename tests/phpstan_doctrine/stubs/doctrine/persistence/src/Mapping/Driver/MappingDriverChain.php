<?php

declare(strict_types=1);

namespace Doctrine\Persistence\Mapping\Driver;

use Doctrine\Persistence\Mapping\ClassMetadata;
use Doctrine\Persistence\Mapping\MappingException;

use function array_keys;
use function spl_object_id;
use function str_starts_with;

/**
 * The DriverChain allows you to add multiple other mapping drivers for
 * certain namespaces.
 *
 * @final since 4.2
 */
class MappingDriverChain implements MappingDriver
{
    /**
     * The default driver.
     */
    private MappingDriver|null $defaultDriver = null;

    /** @var array<string, MappingDriver> */
    private array $drivers = [];

    /** Gets the default driver. */
    public function getDefaultDriver(): MappingDriver|null
    {}

    /** Set the default driver. */
    public function setDefaultDriver(MappingDriver $driver): void
    {}

    /** Adds a nested driver. */
    public function addDriver(MappingDriver $nestedDriver, string $namespace): void
    {}

    /**
     * Gets the array of nested drivers.
     *
     * @return array<string, MappingDriver> $drivers
     */
    public function getDrivers(): array
    {}

    public function loadMetadataForClass(string $className, ClassMetadata $metadata): void
    {}

    /**
     * {@inheritDoc}
     */
    public function getAllClassNames(): array
    {}

    public function isTransient(string $className): bool
    {}
}
