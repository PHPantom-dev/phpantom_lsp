<?php declare(strict_types = 1);

namespace Bug9384b;

use function PHPStan\Testing\assertType;

class Binary
{
	private const TYPE_NONE               = 0;
	private const TYPE_TRACK_DEPRECATIONS = 1;
	private const TYPE_TRIGGER_ERROR      = 2;

	/** @var int-mask-of<self::TYPE_*> */
	private static $type = 0;

	public static function binaryOr(): void
	{
		assertType('int<0, 3>', self::$type);
		assertType('1', self::TYPE_TRACK_DEPRECATIONS);
		assertType('1|3', self::$type | self::TYPE_TRACK_DEPRECATIONS);
		assertType('1|3', self::TYPE_TRACK_DEPRECATIONS | self::$type);
	}

	public static function binaryAnd(): void
	{
		assertType('int<0, 3>', self::$type);
		assertType('1', self::TYPE_TRACK_DEPRECATIONS);
		assertType('0|1', self::$type & self::TYPE_TRACK_DEPRECATIONS);
		assertType('0|1', self::TYPE_TRACK_DEPRECATIONS & self::$type);
	}

	public static function binaryXor(): void
	{
		assertType('int<0, 3>', self::$type);
		assertType('1', self::TYPE_TRACK_DEPRECATIONS);
		assertType('0|1|2|3', self::$type ^ self::TYPE_TRACK_DEPRECATIONS);
		assertType('0|1|2|3', self::TYPE_TRACK_DEPRECATIONS ^ self::$type);
	}
}
