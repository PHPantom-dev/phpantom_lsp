<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use JsonSerializable;

final class JsonSerializableObject implements JsonSerializable
{
    public function jsonSerialize()
    {
        return null;
    }
}

final class EmptyObject
{
}
