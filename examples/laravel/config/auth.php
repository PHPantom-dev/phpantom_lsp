<?php

use App\Models\Customer;

return [
    'defaults' => [
        'guard' => 'web',
    ],

    'guards' => [
        'web' => [
            'driver' => 'session',
            'provider' => 'users',
        ],
        'admin' => [
            'driver' => 'session',
            'provider' => 'admins',
        ],
    ],

    'providers' => [
        'users' => [
            'driver' => 'eloquent',
            'model' => Customer::class,
        ],
        'admins' => [
            'driver' => 'eloquent',
            'model' => App\Models\Administrator::class,
        ],
    ],
];
