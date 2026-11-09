<?php

declare(strict_types=1);

use App\Blog\InMemoryPostRepository;
use App\Blog\PostRepositoryInterface;
use Psr\Log\LoggerInterface;
use Yiisoft\Definitions\Reference;

/** @var array $params */

return [
    // Try: go-to-definition on InMemoryPostRepository. The definition binds
    // the interface PostController asks for to this class, passes the
    // constructor's $posts argument by name, calls setLogger(), and sets
    // the public $pageSize property. Go to the definition of
    // '__construct()', 'posts', 'setLogger()', or '$pageSize', rename setLogger() in
    // InMemoryPostRepository and watch the key follow, clear a key to
    // complete it, or misspell one to see it reported. Drop the $ from
    // '$pageSize' to see the key reported with a fix that puts it back, or
    // set it to '5' to see the value reported: the property takes an int.
    PostRepositoryInterface::class => [
        'class' => InMemoryPostRepository::class,
        '__construct()' => [
            'posts' => $params['blog']['posts'],
        ],
        'setLogger()' => [Reference::to(LoggerInterface::class)],
        '$pageSize' => 5,
    ],
];
