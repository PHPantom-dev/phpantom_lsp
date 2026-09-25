<?php declare(strict_types = 1);

namespace Bug9045;

use function PHPStan\Testing\assertType;

interface TranslationInterface {}
interface TransportTranslationInterface extends TranslationInterface {
    public function getAdditionalInformation(): ?string;
}

/**
 * @template T of TransportTranslationInterface
 * @extends TranslatableInterface<T>
 */
interface TransportInterface extends TranslatableInterface {}

/**
 * @template T of TranslationInterface
 */
interface TranslatableInterface
{
    /** @phpstan-return T */
    public function getTranslation(): TranslationInterface;
}

class Foo {
	public function bar(TransportInterface $transport): void {
		assertType('Bug9045\TransportTranslationInterface', $transport->getTranslation());
		$transport->getTranslation()->getAdditionalInformation();
	}
}
