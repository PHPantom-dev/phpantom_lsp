<?php

declare(strict_types=1);

use Yiisoft\Aliases\Aliases;
use Yiisoft\Translator\CategorySource;
use Yiisoft\Translator\Message\Php\MessageSource;
use Yiisoft\Translator\SimpleMessageFormatter;

/** @var array $params */

// Each category is a file per locale under resources/messages/, named after
// the category: app.php for the translator's default category, which
// yiisoft/translator's own params set to 'app', and blog.php for 'blog'.
return [
    'translation.app' => [
        'definition' => static function (Aliases $aliases) use ($params): CategorySource {
            $messages = new MessageSource($aliases->get('@messages'));

            return new CategorySource(
                $params['yiisoft/translator']['defaultCategory'],
                $messages,
                new SimpleMessageFormatter(),
            );
        },
        'tags' => ['translation.categorySource'],
    ],
    'translation.blog' => [
        'definition' => static function (Aliases $aliases): CategorySource {
            return new CategorySource(
                'blog',
                new MessageSource($aliases->get('@messages')),
                new SimpleMessageFormatter(),
            );
        },
        'tags' => ['translation.categorySource'],
    ],
];
