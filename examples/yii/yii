#!/usr/bin/env php
<?php

declare(strict_types=1);

use App\Environment;
use Yiisoft\Yii\Runner\Console\ConsoleApplicationRunner;

require_once __DIR__ . '/src/bootstrap.php';

// Run console application runner
$runner = new ConsoleApplicationRunner(
    rootPath: __DIR__,
    debug: Environment::appDebug(),
    checkEvents: Environment::appDebug(),
    environment: Environment::appEnv(),
);
$runner->run();
