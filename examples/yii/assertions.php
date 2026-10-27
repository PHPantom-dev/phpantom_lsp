<?php
/**
 * Yii Demo Assertions
 *
 * Run: php examples/yii/assertions.php
 *
 * These assertions build the demo's configuration with yiisoft/config and
 * its DI container the way the web runner does, then check that every name
 * the demo declares, and every claim its "Try:" comments make, holds at
 * runtime. PHPantom reads the same files statically, so a name that only it
 * believes in is a bug in the demo. Requires no web server.
 */

require_once __DIR__ . '/vendor/autoload.php';

use App\Blog\InMemoryPostRepository;
use App\Blog\Post;
use App\Blog\PostRepositoryInterface;
use App\Web\Blog\PostController;
use Psr\Http\Message\ResponseInterface;
use Psr\Log\LoggerInterface;
use Yiisoft\Aliases\Aliases;
use Yiisoft\Router\RouteCollectionInterface;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\Yii\Runner\Http\HttpApplicationRunner;
use Yiisoft\Yii\View\Renderer\WebViewRenderer;

$passed = 0;
$failed = 0;

function check(string $label, bool $condition): void
{
    global $passed, $failed;
    if ($condition) {
        $passed++;
    } else {
        $failed++;
        echo "FAIL: $label\n";
    }
}

/**
 * The value of a private property, for state the class does not expose.
 */
function privateProperty(object $object, string $name): mixed
{
    return (new ReflectionProperty($object, $name))->getValue($object);
}

// The web runner reads config/configuration.php through the merge plan
// `composer install` writes, then builds the container from the di-web group.
$runner = new HttpApplicationRunner(rootPath: __DIR__, environment: 'dev');
$container = $runner->getContainer();

// ─── Aliases ────────────────────────────────────────────────────────────────

$aliases = $container->get(Aliases::class);
check('@views is declared in config/common/aliases.php', $aliases->get('@views') === __DIR__ . '/resources/views');
check('@views/blog is a directory', is_dir($aliases->get('@views/blog')));

// ─── Routes ─────────────────────────────────────────────────────────────────

$routes = $container->get(RouteCollectionInterface::class);
$index = $routes->getRoute('blog/index');
$post = $routes->getRoute('blog/post');
check("The group's prefix makes blog/index's pattern /blog", $index->getData('pattern') === '/blog');
check("The group's prefix makes blog/post's pattern /blog/{slug}", $post->getData('pattern') === '/blog/{slug}');
check('blog/index is handled by PostController::index()', $index->getData('enabledMiddlewares') === [[PostController::class, 'index']]);
check('blog/post is handled by PostController::view()', $post->getData('enabledMiddlewares') === [[PostController::class, 'view']]);
check('PostController declares index()', method_exists(PostController::class, 'index'));
check('PostController declares view()', method_exists(PostController::class, 'view'));
check('The home route is still declared', $routes->getRoute('home')->getData('pattern') === '/');

$urls = $container->get(UrlGeneratorInterface::class);
check("generate('blog/index') builds its URL", $urls->generate('blog/index') === '/blog');
check("generate('blog/post', ['slug' => ...]) builds its URL", $urls->generate('blog/post', ['slug' => 'hello-yii']) === '/blog/hello-yii');

// ─── DI container ───────────────────────────────────────────────────────────

// config/common/di/blog.php binds the interface, passes the constructor
// argument by name, calls setLogger(), and sets $pageSize.
$repository = $container->get(PostRepositoryInterface::class);
check('PostRepositoryInterface is bound to InMemoryPostRepository', $repository instanceof InMemoryPostRepository);
check("'__construct()' passes the posts from params", array_keys(privateProperty($repository, 'posts')) === ['hello-yii', 'config-is-code']);
check("'setLogger()' is a method the class declares", method_exists(InMemoryPostRepository::class, 'setLogger'));
check("'setLogger()' hands it the container's logger", privateProperty($repository, 'logger') === $container->get(LoggerInterface::class));
check("'\$pageSize' sets the public property", $repository instanceof InMemoryPostRepository && $repository->pageSize === 5);
check('findBySlug() returns a Post', $repository->findBySlug('hello-yii') instanceof Post);
check('latest() returns a list of Post', array_is_list($repository->latest()) && $repository->latest()[0] instanceof Post);

// ─── Views ──────────────────────────────────────────────────────────────────

// PostController renders against @views/blog, so 'index' and 'post' are the
// files in resources/views/blog/, and index.php renders '_card' beside it.
$renderer = $container->get(WebViewRenderer::class)->withViewPath('@views/blog');
check("The 'index' view is resources/views/blog/index.php", $renderer->getViewPath() === __DIR__ . '/resources/views/blog' && is_file(__DIR__ . '/resources/views/blog/index.php'));

$html = $renderer->renderPartialAsString('index', ['posts' => $repository->latest()]);
check("index.php renders '_card' for each post", substr_count($html, '<article>') === 2);
check("index.php links each post through generate('blog/post', ...)", str_contains($html, 'href="/blog/hello-yii"'));

$html = $renderer->renderPartialAsString('post', ['post' => $repository->findBySlug('config-is-code')]);
check("post.php links back through generate('blog/index')", str_contains($html, 'href="/blog"'));

$html = $container->get(WebViewRenderer::class)->renderPartialAsString(__DIR__ . '/src/Web/HomePage/template');
check("The home page links to generate('blog/index')", str_contains($html, 'href="/blog"'));

// ─── Handlers ───────────────────────────────────────────────────────────────

$controller = $container->get(PostController::class);
check('PostController::index() returns a response', $controller->index() instanceof ResponseInterface);
check('PostController::view() answers 200 for a known slug', $controller->view('hello-yii')->getStatusCode() === 200);

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
