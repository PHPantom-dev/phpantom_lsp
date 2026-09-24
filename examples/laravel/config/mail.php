<?php

return [
    'default' => 'transactional',

    'mailers' => [
        'transactional' => [
            'transport' => 'log',
            'channel' => 'daily',
        ],
    ],
];
