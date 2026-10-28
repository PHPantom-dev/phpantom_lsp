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
    // the public $pageSize property.
    PostRepositoryInterface::class => [
        'class' => InMemoryPostRepository::class,
        '__construct()' => [
            'posts' => $params['blog']['posts'],
        ],
        'setLogger()' => [Reference::to(LoggerInterface::class)],
        '$pageSize' => 5,
    ],
];
