<?php declare(strict_types = 1);

namespace Bug13385b;

use function PHPStan\Testing\assertType;

interface Operator {
	public function priority(): int;
	public function calculate(int $a, int $b): int;
}

class HelloWorld
{
	/**
	 * @param list<Operator|int> $children
	 */
	public function calculate(array $children): int {
		$operands  = [];
		$operators = [];

		foreach ($children as $child) {
			if ($child instanceof Operator) {
				while ($operators !== []) {
					$op    = array_pop($operators);
					$left  = array_pop($operands) ?? 0;
					$right = array_pop($operands) ?? 0;

					assert(is_int($left));
					assert(is_int($right));

					$value = $op->calculate($left, $right);

					assertType(Operator::class, $op);
					assertType('int', $left);
					assertType('int', $right);
					assertType('int', $value);

					$operands[] = $value;

					assertType('non-empty-list<int>', $operands);
				}

				$operators[] = $child;
			} else {
				$operands[] = $child;
			}
		}

		return count($operands) === 1 ? reset($operands) : 0;
	}
}
