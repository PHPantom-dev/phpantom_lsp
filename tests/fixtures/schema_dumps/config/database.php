<?php

return [
    'default' => env('DB_CONNECTION', 'primary'),
    'connections' => [
        'primary' => [
            'driver' => 'pgsql',
        ],
        'analytics' => [
            'driver' => 'mysql',
        ],
        'archive' => [
            'driver' => 'sqlite',
        ],
        'pgsql_types' => [
            'driver' => 'pgsql',
        ],
        'mysql_types' => [
            'driver' => 'mysql',
        ],
        'sqlite_types' => [
            'driver' => 'sqlite',
        ],
    ],
];
