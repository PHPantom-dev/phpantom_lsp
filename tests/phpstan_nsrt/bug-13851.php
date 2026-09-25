<?php declare(strict_types=1);

namespace Bug13851;

use function PHPStan\Testing\assertType;

class Transaction
{
	public function __construct(public int $value)
	{
	}

	public function withValue(int $value): self
	{
		return new self($value);
	}
}

/**
 * @return array<string, non-empty-list<Transaction>>
 */
function getPositions(): array
{
	return ['AAPL' => [new Transaction(50)], 'TSLA' => [new Transaction(50), new Transaction(100)]];
}

$positions = getPositions();

foreach ($positions as $symbol => &$transactions) {
	foreach ($transactions as &$transaction) {
		$transaction = $transaction->withValue(60);
	}
}

assertType('array<string, non-empty-list<Bug13851\Transaction>>', $positions);
