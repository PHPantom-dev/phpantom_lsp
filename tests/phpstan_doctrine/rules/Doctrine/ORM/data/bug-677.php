<?php // lint >= 8.1

namespace PHPStan\Rules\Doctrine\ORM\Bug677;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class MyBrokenEntity
{
	public const LOGIN_METHOD_BASIC_AUTH = 'BasicAuth';
	public const LOGIN_METHOD_SSO = 'SSO';
	public const LOGIN_METHOD_SAML = 'SAML';

	public const LOGIN_METHODS = [
		self::LOGIN_METHOD_BASIC_AUTH,
		self::LOGIN_METHOD_SSO,
		self::LOGIN_METHOD_SAML,
	];

	/**
	 * @var int|null
	 */
	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column(type: 'integer')]
	private $id;

	/**
	 * @var self::LOGIN_METHOD_*
	 */
	#[ORM\Column(name: 'login_method', type: 'enum', options: ['default' => self::LOGIN_METHOD_BASIC_AUTH, 'values' => self::LOGIN_METHODS])]
	private string $loginMethod = self::LOGIN_METHOD_BASIC_AUTH;
}
