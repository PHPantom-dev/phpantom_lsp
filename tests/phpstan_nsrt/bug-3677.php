<?php declare(strict_types=1);

namespace Bug3677;

use function PHPStan\Testing\assertType;

class Field
{
	/**
	 * @var string
	 */
	private $value = '';

	public function __construct(string $value)
	{
		$this->value = $value;
	}

	public function getValue(): string
	{
		return $this->value;
	}
}

class HelloWorld
{
	/**
	 * @return (Field|null)[]
	 */
	public function getValue(): array
	{
		$first = null;
		$second = null;
		if (isset($_POST['first']) && is_string($_POST['first']) && !empty($_POST['first'])) {
			$first = new Field($_POST['first']);
		}
		if (isset($_POST['second']) && is_string($_POST['second']) && !empty($_POST['second'])) {
			$second = new Field($_POST['second']);
		}
		return [$first, $second];
	}

	public function sayHello(): void
	{
		[$first, $second] = $this->getValue();
		if (!$first && !$second) {
			echo 'empty';
		} elseif (!$first) {
			assertType(Field::class, $second);
		} elseif (!$second) {
			assertType(Field::class, $first);
			echo $first->getValue();
		} else {
			assertType(Field::class, $first);
			assertType(Field::class, $second);
			echo $first->getValue() . "\n" . $second->getValue();
		}
	}

	public function sayGoodbye(): void
	{
		[$first, $second] = $this->getValue();
		if ($first || $second) {
			assertType(Field::class, $first ?: $second);
			assertType(Field::class, $first ?? $second);
		}
	}

	public function sayGoodbye2(): void
	{
		[$first, $second] = $this->getValue();
		if ($first || $second) {
			assertType(Field::class, $first ? $first : $second);
			assertType(Field::class, $first ?? $second);
		}
	}
}
