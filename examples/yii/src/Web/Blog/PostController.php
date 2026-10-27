<?php

declare(strict_types=1);

namespace App\Web\Blog;

use App\Blog\PostRepositoryInterface;
use Psr\Http\Message\ResponseFactoryInterface;
use Psr\Http\Message\ResponseInterface;
use Yiisoft\Http\Status;
use Yiisoft\Router\HydratorAttribute\RouteArgument;
use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final readonly class PostController
{
    private WebViewRenderer $viewRenderer;

    public function __construct(
        WebViewRenderer $viewRenderer,
        private PostRepositoryInterface $posts,
        private ResponseFactoryInterface $responseFactory,
    ) {
        // '@views' is declared in config/common/aliases.php, so the views
        // below are the files in resources/views/blog/.
        $this->viewRenderer = $viewRenderer->withViewPath('@views/blog');
    }

    public function index(): ResponseInterface
    {
        // Try: go to the definition of 'index', which opens
        // resources/views/blog/index.php, or clear the name and complete it
        // from the views in that directory.
        return $this->viewRenderer->render('index', ['posts' => $this->posts->latest()]);
    }

    public function view(#[RouteArgument('slug')] string $slug): ResponseInterface
    {
        // Try: hover $post, or complete after $post->. The container hands
        // the constructor whatever config/common/di/blog.php binds to
        // PostRepositoryInterface, and the interface declares the type.
        $post = $this->posts->findBySlug($slug);
        if ($post === null) {
            return $this->responseFactory->createResponse(Status::NOT_FOUND);
        }

        return $this->viewRenderer->render('post', ['post' => $post]);
    }
}
