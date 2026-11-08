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
use App\Catalog\Author;
use App\Catalog\Book;
use App\Catalog\BookCatalog;
use App\Blog\Post;
use App\Blog\PostRepositoryInterface;
use App\Blog\PostViewed;
use App\Blog\ViewCounter;
use App\Web\Blog\PostController;
use Psr\EventDispatcher\EventDispatcherInterface;
use Psr\Http\Message\ResponseInterface;
use Psr\Log\LoggerInterface;
use Yiisoft\Aliases\Aliases;
use Yiisoft\Db\Connection\ConnectionInterface;
use Yiisoft\Injector\Injector;
use Yiisoft\Injector\MissingRequiredArgumentException;
use Yiisoft\Router\HydratorAttribute\RouteArgument;
use Yiisoft\Router\RouteCollectionInterface;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\Translator\TranslatorInterface;
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

// ─── Params ─────────────────────────────────────────────────────────────────

// config/common/di/application.php reads $params['application'][...], which
// config/common/params.php requires from config/common/application.php.
$params = $runner->getConfig()->get('params-web');
$application = require __DIR__ . '/config/common/application.php';
check("\$params['application'] is what config/common/application.php returns", $params['application'] === $application);
check("\$params['application'] declares 'name', 'charset', and 'locale'", array_keys($application) === ['charset', 'locale', 'name']);
check("\$params['application']['name'] is a string", is_string($params['application']['name']));
check("\$params['blog']['posts'] reaches the repository's constructor", array_keys($params['blog']['posts']) === ['hello-yii', 'config-is-code']);

// ─── config-plugin ──────────────────────────────────────────────────────────

// vendor/yiisoft/router/composer.json lists config/di.php and
// config/params.php under its source directory, and the container builds
// what config/di.php binds.
$router = json_decode(file_get_contents(__DIR__ . '/vendor/yiisoft/router/composer.json'), true, flags: JSON_THROW_ON_ERROR);
$routerConfig = __DIR__ . '/vendor/yiisoft/router/' . $router['extra']['config-plugin-options']['source-directory'];
check("yiisoft/router's config-plugin lists 'di' and 'params'", array_keys($router['extra']['config-plugin']) === ['di', 'params']);
check("Every file yiisoft/router's config-plugin lists is under its source directory", array_filter($router['extra']['config-plugin'], fn (string $file) => !is_file("$routerConfig/$file")) === []);
check("The container builds what yiisoft/router's 'di.php' binds", $container->get(\Yiisoft\Router\RouteCollectorInterface::class) instanceof \Yiisoft\Router\RouteCollector);

// ─── Aliases ────────────────────────────────────────────────────────────────

$aliases = $container->get(Aliases::class);
check('@views is declared in config/common/aliases.php', $aliases->get('@views') === __DIR__ . '/resources/views');
check('@views/blog is a directory', is_dir($aliases->get('@views/blog')));
check('@assetsSource is declared in config/common/aliases.php', $aliases->get('@assetsSource') === __DIR__ . '/assets');
check("MainAsset's @assetsSource/main is a directory", is_dir($aliases->get('@assetsSource/main')));
$mainAsset = new \App\Web\Shared\Layout\Main\MainAsset();
foreach ($mainAsset->css as $entry) {
    check("MainAsset's '$entry' is a file under its \$sourcePath", is_file($aliases->get($mainAsset->sourcePath) . '/' . $entry));
}

// ─── Routes ─────────────────────────────────────────────────────────────────

$routes = $container->get(RouteCollectionInterface::class);
$index = $routes->getRoute('blog/index');
$post = $routes->getRoute('blog/post');
check("The group's prefix makes blog/index's pattern /blog", $index->getData('pattern') === '/blog');
check("The group's prefix makes blog/post's pattern /blog/{slug}", $post->getData('pattern') === '/blog/{slug}');
check('blog/index is handled by PostController::index()', $index->getData('enabledMiddlewares') === [[PostController::class, 'index']]);
check('blog/post is handled by PostController::view()', $post->getData('enabledMiddlewares') === [[PostController::class, 'view']]);
check('PostController declares index()', method_exists(PostController::class, 'index'));
$slugArgument = (new ReflectionMethod(PostController::class, 'view'))->getParameters()[0]->getAttributes(RouteArgument::class);
check("view()'s #[RouteArgument('slug')] reads a placeholder blog/post's path declares", count($slugArgument) === 1 && $slugArgument[0]->newInstance()->getName() === 'slug' && str_contains($post->getData('pattern'), '{slug}'));
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
check("'posts' names a constructor parameter", array_map(fn (ReflectionParameter $p) => $p->getName(), (new ReflectionMethod(InMemoryPostRepository::class, '__construct'))->getParameters()) === ['posts']);
check("'\$pageSize' names a public property", (new ReflectionProperty(InMemoryPostRepository::class, 'pageSize'))->isPublic());
check('config/common/di/logger.php binds LoggerInterface', $container->get(LoggerInterface::class) instanceof \Yiisoft\Log\Logger);
check("'setLogger()' is a method the class declares", method_exists(InMemoryPostRepository::class, 'setLogger'));
check("'setLogger()' hands it the container's logger", privateProperty($repository, 'logger') === $container->get(LoggerInterface::class));
check("'\$pageSize' sets the public property", $repository instanceof InMemoryPostRepository && $repository->pageSize === 5);
$withoutKind = null;
try {
    \Yiisoft\Definitions\Helpers\DefinitionValidator::validate(['class' => InMemoryPostRepository::class, 'pageSize' => 5]);
} catch (\Yiisoft\Definitions\Exception\InvalidConfigException $e) {
    $withoutKind = $e->getMessage();
}
check("A 'pageSize' key is rejected, and '\$pageSize' is the key it means", $withoutKind !== null && str_contains($withoutKind, 'Did you mean "$pageSize"?'));
$stringPageSize = false;
try {
    \Yiisoft\Definitions\ArrayDefinition::fromConfig(['class' => InMemoryPostRepository::class, '__construct()' => ['posts' => []], '$pageSize' => '5'])->resolve($container);
} catch (TypeError) {
    $stringPageSize = true;
}
check("'\$pageSize' => '5' fails: the property takes an int", $stringPageSize);
check('findBySlug() returns a Post', $repository->findBySlug('hello-yii') instanceof Post);
check('latest() returns a list of Post', array_is_list($repository->latest()) && $repository->latest()[0] instanceof Post);

// ─── Views ──────────────────────────────────────────────────────────────────

// PostController renders against @views/blog, so 'index' and 'post' are the
// files in resources/views/blog/, and index.php renders '_card' beside it.
$renderer = $container->get(WebViewRenderer::class)->withViewPath('@views/blog');
check("The 'index' view is resources/views/blog/index.php", $renderer->getViewPath() === __DIR__ . '/resources/views/blog' && is_file(__DIR__ . '/resources/views/blog/index.php'));

check("'_card' is the file beside index.php", is_file(__DIR__ . '/resources/views/blog/_card.php'));
check("__DIR__ . '/template' is template.php beside the home page action", is_file(__DIR__ . '/src/Web/HomePage/template.php'));

$html = $renderer->renderPartialAsString('index', ['posts' => $repository->latest(), 'title' => 'Blog']);
check("index.php renders '_card' for each post", substr_count($html, '<article>') === 2);
check("index.php links each post through generate('blog/post', ...)", str_contains($html, 'href="/blog/hello-yii"'));

$html = $renderer->renderPartialAsString('post', ['post' => $repository->findBySlug('config-is-code')]);
check("post.php links back through generate('blog/index')", str_contains($html, 'href="/blog"'));
check("post.php gives the link the class 'back-link'", str_contains($html, 'class="back-link"'));
check("post.php translates 'blog.all_posts' in the 'blog' category", str_contains($html, '>All posts</a>'));
check("assets/main/site.css declares .back-link", preg_match('/^\.back-link\s*\{/m', (string) file_get_contents(__DIR__ . '/assets/main/site.css')) === 1);

check("withLayout('@src/Web/Shared/Layout/Main/layout') is layout.php", is_file(__DIR__ . '/src/Web/Shared/Layout/Main/layout.php'));

$html = $container->get(WebViewRenderer::class)->renderPartialAsString(__DIR__ . '/src/Web/HomePage/template');
check("The home page links to generate('blog/index')", str_contains($html, 'href="/blog"'));
check("The home page translates 'home.greeting' in the default category", str_contains($html, '<h1>Hello!</h1>'));

// ─── Translations ───────────────────────────────────────────────────────────

// config/common/di/translator.php gives both categories a MessageSource
// reading resources/messages/ through the @messages alias.
$translator = $container->get(TranslatorInterface::class);
check('@messages is resources/messages', $aliases->get('@messages') === __DIR__ . '/resources/messages');
check("The translator's default category is 'app'", $params['yiisoft/translator']['defaultCategory'] === 'app');
check("translate('home.greeting') reads resources/messages/en/app.php", $translator->translate('home.greeting') === 'Hello!');
check("translate('blog.all_posts', category: 'blog') reads blog.php", $translator->translate('blog.all_posts', category: 'blog') === 'All posts');
check("withDefaultCategory('blog') makes translate('blog.title') read blog.php", $translator->withDefaultCategory('blog')->translate('blog.title') === 'Blog');
check("The 'de' files translate the same IDs", $translator->translate('home.greeting', locale: 'de') === 'Hallo!');
foreach (['app', 'blog'] as $category) {
    check("en/$category.php and de/$category.php declare the same IDs", array_keys(require __DIR__ . "/resources/messages/en/$category.php") === array_keys(require __DIR__ . "/resources/messages/de/$category.php"));
}
check("An ID no file declares is translated as itself", $translator->translate('home.missing') === 'home.missing');

// ─── Events ─────────────────────────────────────────────────────────────────

// config/configuration.php lists config/common/events.php under 'events',
// which 'events-web' adds, and yiisoft/yii-event attaches its listeners to
// the dispatcher the container builds.
$events = $runner->getConfig()->get('events-web');
check('The events-web group attaches two listeners to PostViewed', count($events[PostViewed::class]) === 2);
check("[ViewCounter::class, 'count'] names a public method", (new ReflectionMethod(ViewCounter::class, 'count'))->isPublic());
$container->get(EventDispatcherInterface::class)->dispatch(new PostViewed('hello-yii'));
check('Dispatching PostViewed calls ViewCounter::count() on the shared instance', $container->get(ViewCounter::class)->viewsOf('hello-yii') === 1);
$injector = new Injector($container);
$threw = false;
try {
    $injector->invoke([new ViewCounter(), 'viewsOf'], [new PostViewed('hello-yii')]);
} catch (MissingRequiredArgumentException) {
    $threw = true;
}
check("ViewCounter::viewsOf()'s string \$slug cannot be filled by the event", $threw);
$threw = false;
try {
    $injector->invoke(static fn ($event) => $event, [new PostViewed('hello-yii')]);
} catch (MissingRequiredArgumentException) {
    $threw = true;
}
check('A listener parameter with no type cannot be filled by the event', $threw);
check('A listener takes the event in any parameter its class fits', $injector->invoke(static fn (LoggerInterface $logger, PostViewed $event) => $event->slug, [new PostViewed('hello-yii')]) === 'hello-yii');

// ─── Handlers ───────────────────────────────────────────────────────────────

$controller = $container->get(PostController::class);
check('PostController::index() returns a response', $controller->index() instanceof ResponseInterface);
check('PostController::view() answers 200 for a known slug', $controller->view('hello-yii')->getStatusCode() === 200);

// ─── Active Record ──────────────────────────────────────────────────────────

// The bootstrap group runs yiisoft/active-record's bootstrap, which hands the
// connection config/common/di/db.php defines to the records.
foreach ($runner->getConfig()->get('bootstrap-web') as $bootstrap) {
    $bootstrap($container);
}
$db = $container->get(ConnectionInterface::class);
$db->createCommand('CREATE TABLE author (id INTEGER PRIMARY KEY, name TEXT NOT NULL)')->execute();
$db->createCommand('CREATE TABLE book (id INTEGER PRIMARY KEY, title TEXT NOT NULL, author_id INTEGER NOT NULL)')->execute();
$db->createCommand("INSERT INTO author (id, name) VALUES (1, 'Ursula')")->execute();
$db->createCommand("INSERT INTO book (id, title, author_id) VALUES (1, 'Earthsea', 1), (2, 'The Dispossessed', 1)")->execute();

check("Book declares the relation 'author' with getAuthorQuery()", (new Book())->relationNames() === ['author']);
check("Author declares the relation 'books' in relationQuery()", (new Author())->relationQuery('books')->getModel() instanceof Book);
try {
    (new Author())->relationQuery('autor');
    check("Author declares no relation 'autor'", false);
} catch (InvalidArgumentException) {
    check("Author declares no relation 'autor'", true);
}

$catalog = new BookCatalog();
$book = $catalog->find(1);
check("Book::query()->with('author')->one() is a Book", $book instanceof Book);
check("with('author') loads the author", $book?->isRelationPopulated('author') === true);
check("relation('author') is an Author", $book?->relation('author') instanceof Author);
check("get('title') reads Book::\$title", $book?->get('title') === 'Earthsea');
check('describe() reads the title and the author', $book !== null && $catalog->describe($book) === 'Earthsea by Ursula (Earthsea)');

$authors = $catalog->authorsWithBooks();
$author = $authors[0] ?? null;
check("Author::query()->with('books.author')->all() returns Authors", $author instanceof Author);
check("with('books.author') loads each author's books", $author instanceof Author && count($author->relation('books')) === 2);
$first = $author instanceof Author ? ($author->relation('books')[0] ?? null) : null;
check("with('books.author') loads the author of each book", $first instanceof Book && $first->isRelationPopulated('author'));

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
