<?php

declare(strict_types=1);

use App\Web;
use App\Web\Blog\PostController;
use Yiisoft\Router\Group;
use Yiisoft\Router\Route;

return [
    Group::create()
        ->routes(
            Route::get('/')
                ->action(Web\HomePage\Action::class)
                ->name('home'),
        ),
    // The group prefixes each route's path with /blog and its name with
    // blog/, so the routes below are blog/index at /blog and blog/post at
    // /blog/{slug}.
    Group::create('/blog')
        ->namePrefix('blog/')
        ->routes(
            // Try: go-to-definition on 'index' or 'view', or rename one of
            // them. The string is the handler method PostController
            // declares.
            Route::get('')
                ->action([PostController::class, 'index'])
                ->name('index'),
            // Try: find references on 'post'. The name is blog/post, and
            // the views generate its URL.
            Route::get('/{slug}')
                ->action([PostController::class, 'view'])
                ->name('post'),
        ),
];
