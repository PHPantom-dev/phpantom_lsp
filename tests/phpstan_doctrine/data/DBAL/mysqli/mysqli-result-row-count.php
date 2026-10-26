<?php

namespace MysqliResultRowCount;

use Doctrine\DBAL\Result;
use Doctrine\DBAL\Driver\Result as DriverResult;
use function PHPStan\Testing\assertType;

function (Result $r): void {
	assertType('int|numeric-string', $r->rowCount());
};

function (DriverResult $r): void {
	assertType('int|numeric-string', $r->rowCount());
};
