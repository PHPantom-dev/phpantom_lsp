<?php

declare(strict_types=1);

namespace Doctrine\DBAL\Types;

use Doctrine\DBAL\ParameterType;
use Doctrine\DBAL\Platforms\AbstractPlatform;

/**
 * Type that maps an SQL INT to a PHP integer.
 */
class IntegerType extends Type implements PhpIntegerMappingType
{
    /**
     * {@inheritDoc}
     */
    public function getSQLDeclaration(array $column, AbstractPlatform $platform): string
    {}

    /**
     * @param T $value
     *
     * @return (T is null ? null : int)
     *
     * @template T
     */
    public function convertToPHPValue(mixed $value, AbstractPlatform $platform): ?int
    {}

    public function getBindingType(): ParameterType
    {}
}
