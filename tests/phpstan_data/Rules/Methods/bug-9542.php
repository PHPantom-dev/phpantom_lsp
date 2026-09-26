<?php declare(strict_types = 1);

namespace Bug9542;

use function PHPStan\Testing\assertType;

interface TranslatableInterface
{

}

class TranslatableMessage implements TranslatableInterface
{
	public function getMessage(): void
	{
		return;
	}
}

class HelloWorld
{
	public function testInstanceOf(TranslatableInterface $translatable): void
	{
		if (! $translatable instanceof TranslatableMessage) {
			return;
		}

		assertType('Bug9542\TranslatableMessage', $translatable);
		$translatable->getMessage();
	}

	public function testClass(TranslatableInterface $translatable): void
	{
		if ($translatable::class !== TranslatableMessage::class) {
			assertType('Bug9542\TranslatableInterface', $translatable);
			return;
		}

		assertType('Bug9542\TranslatableMessage', $translatable);
		$translatable->getMessage();
	}

	public function testClassReverse(TranslatableInterface $translatable): void
	{
		if (TranslatableMessage::class !== $translatable::class) {
			assertType('Bug9542\TranslatableInterface', $translatable);
			return;
		}

		assertType('Bug9542\TranslatableMessage', $translatable);
		$translatable->getMessage();
	}
}
