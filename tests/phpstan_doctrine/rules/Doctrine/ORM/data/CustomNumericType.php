<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\DBAL\Platforms\AbstractPlatform;
use Doctrine\DBAL\Types\Type;

class CustomNumericType extends Type
{

	public const NAME = 'custom_numeric';

	public function getSQLDeclaration(array $fieldDeclaration, AbstractPlatform $platform): string
	{
		return '';
	}

	public function getName(): string
	{
		return self::NAME;
	}

	/**
	 * @return numeric-string|null
	 */
	public function convertToPHPValue($value, AbstractPlatform $abstractPlatform): ?string
	{
		return '';
	}

	/**
	 * @param numeric-string $value
	 * @return numeric-string|null
	 */
	public function convertToDatabaseValue($value, AbstractPlatform $abstractPlatform): ?string
	{
		return '';
	}

}
