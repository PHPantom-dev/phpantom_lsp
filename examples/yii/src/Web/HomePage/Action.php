<?php

declare(strict_types=1);

namespace App\Web\HomePage;

use Psr\Http\Message\ResponseInterface;
use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final readonly class Action
{
    public function __construct(
        private WebViewRenderer $viewRenderer,
    ) {}

    public function __invoke(): ResponseInterface
    {
        // Try: go to the definition of '/template'. The renderer adds the
        // .php, so this is template.php beside this class.
        return $this->viewRenderer->render(__DIR__ . '/template');
    }
}
