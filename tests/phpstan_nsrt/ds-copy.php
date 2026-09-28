<?php

namespace DsCopy;

use Ds\Collection;
use Ds\Deque;
use Ds\Map;
use Ds\PriorityQueue;
use Ds\Queue;
use Ds\Sequence;
use Ds\Set;
use Ds\Stack;
use Ds\Vector;

use function PHPStan\Testing\assertType;

final class DsCopy
{
	/**
	 * @param Collection<string, int> $col
	 * @param Sequence<int> $seq
	 * @param Vector<int> $vec
	 * @param Deque<int> $deque
	 * @param Map<string, int> $map
	 * @param Queue<int> $queue
	 * @param Stack<int> $stack
	 * @param PriorityQueue<int> $pq
	 * @param Set<int> $set
	 */
	public function __construct(
		private readonly Collection $col,
		private readonly Sequence $seq,
		private readonly Vector $vec,
		private readonly Deque $deque,
		private readonly Map $map,
		private readonly Queue $queue,
		private readonly Stack $stack,
		private readonly PriorityQueue $pq,
		private readonly Set $set,
	) {
	}

	public function copy(): void
	{
		$col = $this->col->copy();
		$seq = $this->seq->copy();
		$vec = $this->vec->copy();
		$deque = $this->deque->copy();
		$map = $this->map->copy();
		$queue = $this->queue->copy();
		$stack = $this->stack->copy();
		$pq = $this->pq->copy();
		$set = $this->set->copy();

		assertType('Ds\Collection<string, int>', $col);
		assertType('Ds\Sequence<int>', $seq);
		assertType('Ds\Vector<int>', $vec);
		assertType('Ds\Deque<int>', $deque);
		assertType('Ds\Map<string, int>', $map);
		assertType('Ds\Queue<int>', $queue);
		assertType('Ds\Stack<int>', $stack);
		assertType('Ds\PriorityQueue<int>', $pq);
		assertType('Ds\Set<int>', $set);
	}
}
