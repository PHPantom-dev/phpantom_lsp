<?php

declare(strict_types=1);

return [
    '@root' => dirname(__DIR__, 2),
    '@src' => '@root/src',
    '@assets' => '@root/public/assets',
    '@assetsUrl' => '@baseUrl/assets',
    '@assetsSource' => '@root/assets',
    '@baseUrl' => '/',
    '@public' => '@root/public',
    '@runtime' => '@root/runtime',
    '@vendor' => '@root/vendor',
    // The demo's own alias. PostController renders its templates from it.
    '@views' => '@root/resources/views',
];
