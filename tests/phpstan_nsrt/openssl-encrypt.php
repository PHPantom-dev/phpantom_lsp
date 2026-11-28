<?php

declare(strict_types = 1);

namespace OpenSslEncrypt;

use function PHPStan\Testing\assertType;

class Foo
{
	public function testStringCipher(string $cipher): void
	{
		openssl_encrypt('data', $cipher, random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('non-empty-string|null', $tag);
	}

	public function testUnknownCipher(): void
	{
		openssl_encrypt('data', 'aes-256-cde', random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('null', $tag);

		openssl_encrypt('data', 'abc-256-gcm', random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('null', $tag);

		openssl_encrypt('data', 'abc-256-ccm', random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('null', $tag);
	}

	public function testAeadCipher(): void
	{
		$cipher = 'aes-256-gcm';
		openssl_encrypt('data', $cipher, random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('non-empty-string', $tag);

		$cipher = 'aes-256-ccm';
		openssl_encrypt('data', $cipher, random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('non-empty-string', $tag);
	}

	public function testNonAeadCipher(): void
	{
		$cipher = 'aes-256-cbc';
		openssl_encrypt('data', $cipher, random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('null', $tag);
	}

	/**
	 * @param 'aes-256-ctr'|'aes-256-gcm' $cipher
	 */
	public function testMixedAeadAndNonAeadCiphers(string $cipher): void
	{
		openssl_encrypt('data', $cipher, random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('non-empty-string|null', $tag);
	}

	/**
	 * @param 'aes-256-cbc'|'aes-256-ctr' $cipher
	 */
	public function testMixedTwoNonAeadCiphers(string $cipher): void
	{
		openssl_encrypt('data', $cipher, random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('null', $tag);
	}

	/**
	 * @param 'aes-256-gcm'|'aes-256-ccm' $cipher
	 */
	public function testMixedTwoAeadCiphers(string $cipher): void
	{
		openssl_encrypt('data', $cipher, random_bytes(32), OPENSSL_RAW_DATA, random_bytes(16), $tag);
		assertType('non-empty-string', $tag);
	}
}
