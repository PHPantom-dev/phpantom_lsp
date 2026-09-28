<?php

namespace Bug7144ComposerIntegration;

use function PHPStan\Testing\assertType;

Class Foo {
	/** @var mixed[] */
	private static $options = array(
		'http' => array(
			'method' => CURLOPT_CUSTOMREQUEST,
			'content' => CURLOPT_POSTFIELDS,
			'header' => CURLOPT_HTTPHEADER,
			'timeout' => CURLOPT_TIMEOUT,
		),
		'ssl' => array(
			'cafile' => CURLOPT_CAINFO,
			'capath' => CURLOPT_CAPATH,
			'verify_peer' => CURLOPT_SSL_VERIFYPEER,
			'verify_peer_name' => CURLOPT_SSL_VERIFYHOST,
			'local_cert' => CURLOPT_SSLCERT,
			'local_pk' => CURLOPT_SSLKEY,
			'passphrase' => CURLOPT_SSLKEYPASSWD,
		),
	);

	/**
	 * @param array{http: array{header: string[], proxy?: string, request_fulluri: bool}, ssl?: mixed[]} $options
	 */
	public function test3(array $options): void
	{
		$curlHandle = curl_init();
		foreach (self::$options as $type => $curlOptions) {
			foreach ($curlOptions as $name => $curlOption) {
				\PHPStan\Testing\assertType('array{http: array{header: array<string>, proxy?: string, request_fulluri: bool}, ssl?: array<mixed>}', $options);
				if (isset($options[$type][$name])) {
					\PHPStan\Testing\assertType('array{http: array{header: array<string>, proxy?: string, request_fulluri: bool}, ssl?: array<mixed>}', $options);
					if ($type === 'ssl' && $name === 'verify_peer_name') {
						\PHPStan\Testing\assertType('array{http: array{header: array<string>, proxy?: string, request_fulluri: bool}, ssl?: array<mixed>}', $options);
						curl_setopt($curlHandle, $curlOption, $options[$type][$name] === true ? 2 : $options[$type][$name]);
					} else {
						\PHPStan\Testing\assertType('array{http: array{header: array<string>, proxy?: string, request_fulluri: bool}, ssl?: array<mixed>}', $options);
						curl_setopt($curlHandle, $curlOption, $options[$type][$name]);
					}
				}
			}
		}
	}
}
