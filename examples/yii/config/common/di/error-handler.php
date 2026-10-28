<?php

declare(strict_types=1);

use App\Environment;
use Yiisoft\ErrorHandler\Renderer\HtmlRenderer;

/**
 * @var array $params
 */

return [
    HtmlRenderer::class => [
        '__construct()' => [
            'traceLink' => static function (string $file, ?int $line) use ($params): string|null {
                if (!isset($params['traceLink'])) {
                    return null;
                }

                try {
                    $hostPath = Environment::appHostPath();
                    if ($hostPath !== null) {
                        /** @var string $file */
                        $file = preg_replace('~^(/app/)~', rtrim($hostPath, '\\/') . '/', $file);
                    }
                    return str_replace(
                        ['{file}', '{line}'],
                        [$file, (string) $line],
                        $params['traceLink'],
                    );
                } catch (Throwable) {
                    return null;
                }
            },
        ],
    ],
];
