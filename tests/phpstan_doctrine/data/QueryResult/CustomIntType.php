<?php declare(strict_types = 1);

namespace Type\Doctrine\data\QueryResult;

use Doctrine\DBAL\Types\IntegerType;

class CustomIntType extends IntegerType
{

	public const NAME = 'custom_int';

	public function getName(): string
	{
		return self::NAME;
	}

}
