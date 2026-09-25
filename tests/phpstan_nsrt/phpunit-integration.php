<?php declare(strict_types=1);

namespace PHPUnitIntegration;

use Exception;
use function libxml_get_errors;
use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;
use function sprintf;
use DOMDocument;

final class Loader
{
	/**
	 * @throws Exception
	 */
	public function load(string $actual, bool $isHtml = false, string $filename = '', bool $strict = false): DOMDocument
	{
		$document                     = new DOMDocument;
		$message   = '';

		if ($isHtml) {
			$loaded = $document->loadHTML($actual);
		} else {
			$loaded = $document->loadXML($actual);
		}

		foreach (libxml_get_errors() as $error) {
			$message .= "\n" . $error->message;
		}

		if ($loaded === false || ($strict && $message !== '')) {
			assertType('string', $message);
			assertNativeType('string', $message);
			if ($filename !== '') {
				assertType('string', $message);
				assertNativeType('string', $message);
				throw new Exception(
					sprintf(
						'Could not load "%s".%s',
						$filename,
						$message !== '' ? "\n" . $message : ''
					)
				);
			}

			assertType('string', $message);
			assertNativeType('string', $message);

			if ($message === '') {
				$message = 'Could not load XML for unknown reason';
			}

			assertType('non-empty-string', $message);
			assertNativeType('non-empty-string', $message);

			throw new Exception($message);
		}

		return $document;
	}
}
