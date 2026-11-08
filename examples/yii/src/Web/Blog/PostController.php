<?php

declare(strict_types=1);

namespace App\Web\Blog;

use App\Blog\PostRepositoryInterface;
use Psr\Http\Message\ResponseFactoryInterface;
use Psr\Http\Message\ResponseInterface;
use Psr\Http\Message\ServerRequestInterface;
use Yiisoft\FormModel\FormHydrator;
use Yiisoft\Http\Status;
use Yiisoft\Router\HydratorAttribute\RouteArgument;
use Yiisoft\Translator\TranslatorInterface;
use Yiisoft\Yii\View\Renderer\WebViewRenderer;

final readonly class PostController
{
    private WebViewRenderer $viewRenderer;

    private TranslatorInterface $translator;

    public function __construct(
        WebViewRenderer $viewRenderer,
        TranslatorInterface $translator,
        private PostRepositoryInterface $posts,
        private ResponseFactoryInterface $responseFactory,
    ) {
        // '@views' is declared in config/common/aliases.php, so the views
        // below are the files in resources/views/blog/.
        $this->viewRenderer = $viewRenderer->withViewPath('@views/blog');
        $this->translator = $translator->withDefaultCategory('blog');
    }

    public function index(): ResponseInterface
    {
        // Try: go to the definition of 'index', which opens
        // resources/views/blog/index.php, or clear the name and complete it
        // from the views in that directory.
        // Try: go to the definition of 'blog.title', which lands on its
        // entries in resources/messages/*/blog.php. The constructor's
        // withDefaultCategory('blog') makes 'blog' the category this class
        // translates in.
        return $this->viewRenderer->render('index', [
            'posts' => $this->posts->latest(),
            'title' => $this->translator->translate('blog.title'),
        ]);
    }

    // Try: go-to-definition on 'slug', which lands on the {slug} of the
    // route config/common/routes.php points here. Complete it, or rename it
    // to one the path does not declare and see it reported.
    public function view(#[RouteArgument('slug')] string $slug): ResponseInterface
    {
        // Try: hover $post, or complete after $post->. The container hands
        // the constructor whatever config/common/di/blog.php binds to
        // PostRepositoryInterface, and the interface declares the type.
        $post = $this->posts->findBySlug($slug);
        if ($post === null) {
            return $this->responseFactory->createResponse(Status::NOT_FOUND);
        }

        // Try: go to the definition of the layout, which opens
        // src/Web/Shared/Layout/Main/layout.php, or misspell it, or the
        // 'blog' in withViewPath() above, to see it reported.
        return $this->viewRenderer
            ->withLayout('@src/Web/Shared/Layout/Main/layout')
            ->render('post', ['post' => $post, 'comment' => new CommentForm()]);
    }

    public function comment(
        ServerRequestInterface $request,
        FormHydrator $formHydrator,
        #[RouteArgument('slug')] string $slug,
    ): ResponseInterface {
        $post = $this->posts->findBySlug($slug);
        if ($post === null) {
            return $this->responseFactory->createResponse(Status::NOT_FOUND);
        }

        // Try: go to the definition of 'message', which lands on the
        // property in CommentForm.php. The form hydrator fills only the
        // properties a validation rule or the map names, and CommentForm
        // has no rules, so the map lists each property with the posted key
        // it is read from. Clear a key to complete the form's properties,
        // or misspell one to see it reported.
        $comment = new CommentForm();
        $formHydrator->populateFromPost($comment, $request, map: ['author' => 'author', 'message' => 'message']);

        return $this->viewRenderer
            ->withLayout('@src/Web/Shared/Layout/Main/layout')
            ->render('post', ['post' => $post, 'comment' => $comment]);
    }
}
