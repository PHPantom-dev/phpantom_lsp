<?php

namespace PDOPrepare;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$database = new \PDO('dsn');
		$logDeleteQuery = $database->prepare('DELETE FROM log');
		assertType('(PDOStatement|false)', $logDeleteQuery);
	}

}
