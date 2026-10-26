<?php declare(strict_types=1);

namespace QueryResult\Entities;

class ManyId
{
	/** @var string */
	public $id;

	public function __construct(string $id)
	{
		$this->id = $id;
	}
}
