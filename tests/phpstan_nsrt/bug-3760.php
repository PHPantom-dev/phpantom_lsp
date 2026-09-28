<?php declare(strict_types = 1);

namespace Bug3760;

use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * Whether the type allows covariant matches
	 *
	 * @var bool
	 */
	public $allowsCovariance;

	/**
	 * Whether the type allows contravariant matches
	 *
	 * @var bool
	 */
	public $allowsContravariance;

	/**
	 * @param bool $allowsCovariance
	 * @param bool $allowsContravariance
	 */
	protected function __construct($allowsCovariance, $allowsContravariance)
	{
		assertType('bool', $allowsCovariance);
		assertNativeType('mixed', $allowsCovariance);
		assertType('bool', $allowsContravariance);
		assertNativeType('mixed', $allowsContravariance);
		$this->allowsCovariance = (bool)$allowsCovariance;

		assertType('bool', $allowsCovariance);
		assertNativeType('mixed', $allowsCovariance);
		assertType('bool', $allowsContravariance);
		assertNativeType('mixed', $allowsContravariance);
		$this->allowsContravariance = (bool)$allowsContravariance;

		assertType('bool', $allowsCovariance);
		assertNativeType('mixed', $allowsCovariance);
		assertType('bool', $allowsContravariance);
		assertNativeType('mixed', $allowsContravariance);
	}
}
