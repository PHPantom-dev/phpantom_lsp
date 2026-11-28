<?php

declare(strict_types=1);

namespace App;

use Generator;
use CachingIterator;
use function PHPStan\Testing\assertType;

$generator =
    /**
     * @return Generator<bool, bool>
     */
    static function (): Generator {
      yield true => true;
      yield false => false;
    };

$iterator = new CachingIterator($generator(), CachingIterator::FULL_CACHE);
$cache = $iterator->getCache();
assertType('array<bool>', $cache);
