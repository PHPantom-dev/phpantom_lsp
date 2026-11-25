<?php

namespace Bug5293;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public const AUD = 'AUD';
	public const CAD = 'CAD';
	public const CHF = 'CHF';
	public const CZK = 'CZK';
	public const DKK = 'DKK';
	public const EUR = 'EUR';
	public const GBP = 'GBP';
	public const HUF = 'HUF';
	public const NOK = 'NOK';
	public const NZD = 'NZD';
	public const PLN = 'PLN';
	public const SEK = 'SEK';
	public const SGD = 'SGD';
	public const USD = 'USD';
	public const BRL = 'BRL';
	public const RON = 'RON';
	public const BGN = 'BGN';

	/** @var non-empty-string[] **/
	public const SUPPORTED_CURRENCIES = [
		self::EUR,
		self::DKK,
		self::GBP,
		self::HUF,
		self::PLN,
		self::SEK,
		self::USD,
		self::CAD,
		self::AUD,
		self::NZD,
		self::CHF,
		self::NOK,
		self::BRL,
		self::SGD,
		self::CZK,
		self::RON,
		self::BGN,
	];

	public static function getAllSupportedCurrencies(): void
	{
		array_map(
			function (string $currencyCode): int {
				assertType('\'AUD\'|\'BGN\'|\'BRL\'|\'CAD\'|\'CHF\'|\'CZK\'|\'DKK\'|\'EUR\'|\'GBP\'|\'HUF\'|\'NOK\'|\'NZD\'|\'PLN\'|\'RON\'|\'SEK\'|\'SGD\'|\'USD\'', $currencyCode);
				return 1;
			},
			self::SUPPORTED_CURRENCIES
		);
	}
}
