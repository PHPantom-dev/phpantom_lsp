<?php

declare(strict_types=1);

use App\Shared\ApplicationParams;

/** @var array $params */

return [
    // Try: go to the definition of 'name' in $params['application']['name'],
    // which lands on its entry in config/common/application.php, the file
    // config/common/params.php reads the 'application' params from. Hover it
    // for the type of its value, clear a key to complete the ones declared
    // beside it, or misspell one to see it reported.
    ApplicationParams::class => [
        '__construct()' => [
            'name' => $params['application']['name'],
            'charset' => $params['application']['charset'],
            'locale' => $params['application']['locale'],
        ],
    ],
];
