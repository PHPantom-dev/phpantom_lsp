<?php

declare(strict_types=1);

namespace Doctrine\ORM\Cache\Persister;

use Doctrine\Common\Collections\Criteria;
use Doctrine\Common\Collections\Order;
use Doctrine\Common\Collections\ReadableCollection;
use Doctrine\Common\Collections\Selectable;
use Doctrine\ORM\Mapping\ToManyAssociationMapping;
use SortDirection;

use function array_map;
use function is_a;
use function strtoupper;

trait CompatOrderings
{
    /** 3.1 is guaranteed thanks to a Composer constraint */
    private function isCollections31(): bool
    {}

    /**
     * @return array<string, Order>|array<string, SortDirection>
     *
     * @phpstan-ignore return.deprecatedEnum
     */
    private function getOrderings(Criteria $criteria): array
    {}

    /** @return array<string, string> */
    private function getOrderingsAsStringMap(Criteria $criteria): array
    {}

    private function orderCriteriaByAssociation(Criteria $criteria, ToManyAssociationMapping $association): void
    {}
}
