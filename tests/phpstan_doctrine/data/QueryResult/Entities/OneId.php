<?php declare(strict_types=1);

namespace QueryResult\Entities;

class OneId
{
	/** @var string */
	public $id;

	/** @var mixed $ignore */
	public function __construct(string $id, ...$ignore)
	{
		$this->id = $id;
	}
}
