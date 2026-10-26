<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\DBAL\Platforms\AbstractPlatform;
use Doctrine\DBAL\Types\ConversionException;
use Doctrine\DBAL\Types\Type;
use InvalidArgumentException;
use Symfony\Component\Uid\AbstractUid;
use Symfony\Component\Uid\Ulid;
use function is_string;

/**
 * From https://github.com/symfony/doctrine-bridge/blob/f5a2780640342e3bf0599fbfc9b3dd759db9b037/Types/UuidType.php
 * Copyright (c) Fabien Potencier <fabien@symfony.com>
 */
final class FakeTestingSymfonyUlidType extends Type
{

	public const NAME = 'ulid';

	/**
	 * @not-deprecated
	 */
	public function getName(): string
	{
		return self::NAME;
	}

	protected function getUidClass(): string
	{
		return Ulid::class;
	}

	/**
	 * {@inheritdoc}
	 */
	public function getSQLDeclaration(array $column, AbstractPlatform $platform): string
	{
		if ($this->hasNativeGuidType($platform)) {
			return $platform->getGuidTypeDeclarationSQL($column);
		}

		return $platform->getBinaryTypeDeclarationSQL([
			'length' => '16',
			'fixed' => true,
		]);
	}

	/**
	 * {@inheritdoc}
	 *
	 * @throws ConversionException
	 */
	public function convertToPHPValue($value, AbstractPlatform $platform): ?AbstractUid
	{
		if ($value instanceof AbstractUid || $value === null) {
			return $value;
		}

		if (!is_string($value)) {
			throw ConversionException::conversionFailedInvalidType($value, $this->getName(), ['null', 'string', AbstractUid::class]);
		}

		try {
			/** @phpstan-ignore-next-line method.dynamicName */
			return $this->getUidClass()::fromString($value);
		} catch (InvalidArgumentException $e) {
			throw ConversionException::conversionFailed($value, $this->getName(), $e);
		}
	}

	/**
	 * {@inheritdoc}
	 *
	 * @throws ConversionException
	 */
	public function convertToDatabaseValue($value, AbstractPlatform $platform): ?string
	{
		$toString = $this->hasNativeGuidType($platform) ? 'toRfc4122' : 'toBinary';

		if ($value instanceof AbstractUid) {
			/** @phpstan-ignore-next-line method.dynamicName */
			return $value->$toString();
		}

		if ($value === null || $value === '') {
			return null;
		}

		if (!is_string($value)) {
			throw ConversionException::conversionFailedInvalidType($value, $this->getName(), ['null', 'string', AbstractUid::class]);
		}

		try {
			/** @phpstan-ignore-next-line method.dynamicName */
			return $this->getUidClass()::fromString($value)->$toString();
		} catch (InvalidArgumentException $e) {
			throw ConversionException::conversionFailed($value, $this->getName());
		}
	}

	public function requiresSQLCommentHint(AbstractPlatform $platform): bool // @phpstan-ignore return.tooWideBool
	{
		return true;
	}

	private function hasNativeGuidType(AbstractPlatform $platform): bool
	{
		return $platform->getGuidTypeDeclarationSQL([]) !== $platform->getStringTypeDeclarationSQL(['fixed' => true, 'length' => 36]);
	}

}
