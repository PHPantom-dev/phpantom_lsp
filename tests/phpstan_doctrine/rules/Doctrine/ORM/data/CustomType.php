<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use DateTimeImmutable;
use DateTimeInterface;
use Doctrine\DBAL\Platforms\AbstractPlatform;
use Doctrine\DBAL\Types\Type;

class CustomType extends Type
{

	public const NAME = 'custom';

	public function getSQLDeclaration(array $fieldDeclaration, AbstractPlatform $platform): string
	{
		return '';
	}

	public function getName(): string
	{
		return self::NAME;
	}

	public function convertToPHPValue($value, AbstractPlatform $abstractPlatform): ?DateTimeInterface
	{
		return new DateTimeImmutable();
	}

	/**
	 * @param array $value
	 */
	public function convertToDatabaseValue($value, AbstractPlatform $abstractPlatform): ?string
	{
		return '';
	}

}
