<?php

declare(strict_types=1);

namespace Doctrine\ORM\Query\Expr;

use Stringable;

use function implode;
use function is_object;
use function preg_match;

/**
 * Expression class for building DQL and parts.
 *
 * @link    www.doctrine-project.org
 */
class Composite extends Base
{
    public function __toString(): string
    {}

    private function processQueryPart(string|Stringable $part): string
    {}
}
