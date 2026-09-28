<?php declare(strict_types = 1);

namespace Bug9397;

use function PHPStan\Testing\assertType;

final class Money {
	public static function zero(): Money {
		return new Money();
	}
}


class HelloWorld
{
	/**
	 * @return array<int, array{
	 *		foo1: Money,
	 *		foo2: ?Money,
	 *		foo3: string,
	 *		foo4: string,
	 *		foo5: string,
	 *		foo6: string,
	 *		foo7: string,
	 *		foo8: string,
	 *		foo9: string,
	 *		foo10:string,
	 *		foo11:int,
	 *		foo12:int,
	 *		foo13:int,
	 *		foo14:int,
	 *		foo15:int,
	 *		foo16:int,
	 *		foo17:int,
	 *		foo18:int,
	 *		foo19:int,
	 *		foo20:int,
	 *		foo21:bool,
	 *		foo22:bool,
	 *		foo23:bool,
	 *		foo24:bool,
	 *		foo25:bool,
	 *		foo26:bool,
	 *		foo27:bool,
	 *		foo28:bool,
	 *		foo29:bool,
	 *		foo30:bool,
	 *		foo31:bool,
	 *		foo32:string,
	 *		foo33:string,
	 *		foo34:string,
	 *		foo35:string,
	 *		foo36:string,
	 *		foo37:string,
	 *		foo38:string,
	 *		foo39:string,
	 *		foo40:string,
	 *		foo41:string,
	 *		foo42:string,
	 *		foo43:string,
	 *		foo44:string,
	 *		foo45:string,
	 *		foo46:string,
	 *		foo47:string,
	 *  	foo48:string,
	 *		foo49:string,
	 *		foo50:string,
	 *		foo51:string,
	 *		foo52:string,
	 *		foo53:string,
	 *		foo54:string,
	 *		foo55:string,
	 *		foo56:string,
	 *		foo57:string,
	 *		foo58:string,
	 *		foo59:string,
	 *		foo60:string,
	 *		foo61:string,
	 *		foo62:string,
	 *		foo63:string,
	 *	}>
	 *  If the above type has 63 or more properties, the bug occurs
	 */
	private static function callable(): array {
		return [];
	}

	public function callsite(): void {
		$result = self::callable();
		foreach ($result as $id => $p) {
			assertType(Money::class, $p['foo1']);
			assertType(Money::class . '|null', $p['foo2']);
			assertType('string', $p['foo3']);

			$baseDeposit = $p['foo2'] ?? Money::zero();
			assertType(Money::class, $p['foo1']);
			assertType(Money::class . '|null', $p['foo2']);
			assertType('string', $p['foo3']);
		}
	}
}
