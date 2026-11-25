<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug12244;

use function PHPStan\Testing\assertType;

enum X: string {
	case A = 'a';
	case B = 'b';
	case C = 'c';

	/** @return ($this is self::A ? int : null) */
	public function get(): ?int {
		return ($this === self::A) ? 123 : null;
	}

	public function doSomething(): void {
		if ($this !== self::A) {
			// Upstream: `$this(Bug12244\X~Bug12244\X::A)`, the same cases.
			assertType('Bug12244\X::B|Bug12244\X::C', $this);
			assertType('null', $this->get());
		} else {
			assertType('int', $this->get());
		}

		if ($this !== self::B && $this !== self::C) {
			assertType('int', $this->get());
		}
	}

	public static function doSomethingFor(X $x): void {
		if ($x !== self::A) {
			// Upstream: `Bug12244\X~Bug12244\X::A`, the same cases.
			assertType('Bug12244\X::B|Bug12244\X::C', $x);
			assertType('null', $x->get());
		} else {
			assertType('int', $x->get());
		}

		if ($x !== self::B && $x !== self::C) {
			assertType('int', $x->get());
		}
	}
}

enum Y: string {
	case A = 'a';
	case B = 'b';
	case C = 'c';

	/** @param-out ($this is self::A ? int : null) $i */
	public function get(?int &$i): void {
		($this === self::A) ? $i=null : $i=123;
	}

	public function doSomething(): void {
		$i = 0;
		if ($this !== self::A) {
			$this->get($i);
			assertType('null', $i); // null
		} else {
			$this->get($i);
			assertType('int', $i); // int
		}
	}

	public static function doSomethingFor(Y $x): void {
		$i = 0;
		if ($x !== self::A) {
			$x->get($i);
			assertType('null', $i); // null
		} else {
			$x->get($i);
			assertType('int', $i); // int
		}
	}
}
