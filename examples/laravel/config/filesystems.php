<?php

return [

    'default' => 'local',

    'disks' => [

        'local' => [
            'driver' => 'local',
            'root' => 'storage/app',
        ],

        'avatars' => [
            'driver' => 'local',
            'root' => 'storage/app/avatars',
        ],

        'logs' => [
            'driver' => 'local',
            'root' => 'storage/app/logs',
        ],

        's3' => [
            'driver' => 's3',
            'bucket' => 'demo',
        ],

        // A disk whose driver the framework does not ship. It is built by the
        // `Storage::extend('pantry', ...)` registration in DemoServiceProvider,
        // and PHPantom reads that closure's return type rather than giving up
        // on every disk in the project.
        'pantry' => [
            'driver' => 'pantry',
            'root' => 'storage/app/pantry',
        ],

    ],

];
