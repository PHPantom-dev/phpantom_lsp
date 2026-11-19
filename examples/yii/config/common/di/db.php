<?php

declare(strict_types=1);

use Yiisoft\Db\Connection\ConnectionInterface;
use Yiisoft\Db\Sqlite\Connection;
use Yiisoft\Db\Sqlite\Driver;

// yiisoft/active-record's bootstrap hands this connection to the records in
// src/Catalog. The playground keeps its catalog in memory.
return [
    ConnectionInterface::class => [
        'class' => Connection::class,
        '__construct()' => [
            'driver' => new Driver('sqlite::memory:'),
        ],
    ],
];
