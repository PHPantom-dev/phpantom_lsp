<?php // lint >= 8.4

namespace PdoConnectPHP84;

use function PHPStan\Testing\assertType;

/**
 * @param 'mysql:foo'|'pgsql:foo' $mysqlOrPgsql
 * @param 'mysql:foo'|'foo:foo' $mysqlOrFoo
 */
function test(
	string $string,
	string $mysqlOrPgsql,
	string $mysqlOrFoo,
) {
	assertType('PDO\Mysql', \PDO::connect('mysql:foo'));
	assertType('PDO\Firebird', \PDO::connect('firebird:foo'));
	assertType('PDO\Dblib', \PDO::connect('dblib:foo'));
	assertType('PDO\Odbc', \PDO::connect('odbc:foo'));
	assertType('PDO\Pgsql', \PDO::connect('pgsql:foo'));
	assertType('PDO\Sqlite', \PDO::connect('sqlite:foo'));

	assertType('PDO', \PDO::connect($string));
	assertType('PDO\Mysql|PDO\Pgsql', \PDO::connect($mysqlOrPgsql));
	assertType('PDO', \PDO::connect($mysqlOrFoo));
}
