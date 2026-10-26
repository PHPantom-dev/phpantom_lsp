<?php

declare(strict_types=1);

namespace Doctrine\Common\Collections;

use Doctrine\Common\Collections\Expr\CompositeExpression;
use Doctrine\Common\Collections\Expr\Expression;
use Doctrine\Deprecations\Deprecation;

use function array_map;
use function func_get_arg;
use function func_num_args;
use function strtoupper;

/**
 * Criteria for filtering Selectable collections.
 *
 * @phpstan-consistent-constructor
 * @final since 2.5
 */
class Criteria
{
    /** @deprecated use Order::Ascending instead */
    final public const ASC = 'ASC';

    /** @deprecated use Order::Descending instead */
    final public const DESC = 'DESC';

    private static ExpressionBuilder|null $expressionBuilder = null;

    /** @var array<string, Order> */
    private array $orderings = [];

    private int|null $firstResult = null;
    private int|null $maxResults  = null;

    /**
     * Creates an instance of the class.
     *
     * @return static
     */
    public static function create(/* bool $accessRawFieldValues = false */): self
    {}

    /**
     * Returns the expression builder.
     *
     * @return ExpressionBuilder
     */
    public static function expr()
    {}

    /**
     * Construct a new Criteria.
     *
     * @param int|null                         $firstResult
     * @param array<string, string|Order>|null $orderings
     */
    public function __construct(
        private Expression|null $expression = null,
        array|null $orderings = null,
        int|Placeholder|null $firstResult = Placeholder::NotSpecified,
        int|null $maxResults = null,
        private bool $accessRawFieldValues = false,
    ) {}

    /**
     * Sets the where expression to evaluate when this Criteria is searched for.
     *
     * @return $this
     */
    public function where(Expression $expression)
    {}

    /**
     * Appends the where expression to evaluate when this Criteria is searched for
     * using an AND with previous expression.
     *
     * @return $this
     */
    public function andWhere(Expression $expression)
    {}

    /**
     * Appends the where expression to evaluate when this Criteria is searched for
     * using an OR with previous expression.
     *
     * @return $this
     */
    public function orWhere(Expression $expression)
    {}

    /**
     * Gets the expression attached to this Criteria.
     *
     * @return Expression|null
     */
    public function getWhereExpression()
    {}

    /**
     * Gets the current orderings of this Criteria.
     *
     * @deprecated use orderings() instead
     *
     * @return array<string, string>
     */
    public function getOrderings()
    {}

    /**
     * Gets the current orderings of this Criteria.
     *
     * @return array<string, Order>
     */
    public function orderings(): array
    {}

    /**
     * Sets the ordering of the result of this Criteria.
     *
     * Keys are field and values are the order, being a valid Order enum case.
     *
     * @see Order::Ascending
     * @see Order::Descending
     *
     * @param array<string, string|Order> $orderings
     *
     * @return $this
     */
    public function orderBy(array $orderings)
    {}

    /**
     * Gets the current first result option of this Criteria.
     *
     * @return int|null
     */
    public function getFirstResult()
    {}

    /**
     * Set the number of first result that this Criteria should return.
     *
     * @param int|null $firstResult The value to set.
     *
     * @return $this
     */
    public function setFirstResult(int|null $firstResult)
    {}

    /**
     * Gets maxResults.
     *
     * @return int|null
     */
    public function getMaxResults()
    {}

    /**
     * Sets maxResults.
     *
     * @param int|null $maxResults The value to set.
     *
     * @return $this
     */
    public function setMaxResults(int|null $maxResults)
    {}

    /** @internal */
    public function isRawFieldValueAccessEnabled(): bool
    {}
}
