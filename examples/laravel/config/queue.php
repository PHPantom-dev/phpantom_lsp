<?php

return [
    'default' => 'redis',

    'connections' => [
        'redis' => [
            'driver' => 'redis',
            'connection' => 'default',
            'queue' => 'default',
        ],
    ],
];
