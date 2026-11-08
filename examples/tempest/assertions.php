<?php

declare(strict_types=1);

use App\Book;
use App\BookController;
use App\BookView;
use App\ConfiguredViews;
use App\DynamicViews;
use Tempest\Container\GenericContainer;
use Tempest\Discovery\BootDiscovery;
use Tempest\View\ViewComponentDiscovery;
use Tempest\View\Renderers\TempestViewRenderer;
use Tempest\View\ViewConfig;

use function Tempest\View\view;

require __DIR__ . '/vendor/autoload.php';

/** @var ViewConfig $config */
$config = require __DIR__ . '/app/views.config.php';
$discovery = require __DIR__ . '/app/discovery.config.php';
$container = new GenericContainer();
$container->singleton(ViewConfig::class, $config);
new BootDiscovery($container, $discovery)([ViewComponentDiscovery::class]);
if (!isset($config->viewComponents['x-summary'], $config->viewComponents['book-notice'])
    || isset($config->viewComponents['x-private'])) {
    throw new RuntimeException('Configured component discovery produced the wrong names.');
}
$renderer = TempestViewRenderer::make(viewConfig: $config);
GenericContainer::setInstance($container);
$container->singleton(TempestViewRenderer::class, $renderer);

$routes = new Tempest\Router\RouteConfig();
$routeDiscovery = new Tempest\Router\RouteDiscovery(
    new Tempest\Router\Routing\Construction\RouteConfigurator(),
    $routes,
);
$routeDiscovery->setItems(new Tempest\Discovery\DiscoveryItems());
$location = new Tempest\Discovery\DiscoveryLocation('App\\', __DIR__ . '/app');
foreach ([BookController::class, App\HomeController::class] as $controller) {
    $routeDiscovery->discover($location, new Tempest\Reflection\ClassReflector($controller));
}
$routeDiscovery->apply();
$generator = new Tempest\Router\UriGenerator(
    new Tempest\Core\AppConfig(baseUri: 'https://books.example'),
    $routes,
    new Tempest\Cryptography\Signing\GenericSigner(
        new Tempest\Cryptography\Signing\SigningConfig(
            Tempest\Cryptography\Signing\SigningAlgorithm::SHA256,
            'playground',
            false,
        ),
        new Tempest\Cryptography\Timelock(new Tempest\Clock\GenericClock()),
    ),
    $container,
);
$container->singleton(Tempest\Router\UriGenerator::class, $generator);
if (new App\RouteLinks()->links() !== [
    'https://books.example/books/php',
    'https://books.example/books/php',
    'https://books.example/home/en',
    'https://books.example/home/en',
]) {
    throw new RuntimeException('Route discovery or URI arguments did not match the route demos.');
}

$html = $renderer->render(new BookController()->index());
foreach (['<h2>The PHP Book</h2>', '<p>Read The PHP Book</p>'] as $expected) {
    if (!str_contains($html, $expected)) {
        throw new RuntimeException("Missing rendered content: {$expected}");
    }
}
if (str_contains($html, 'No books yet.')) {
    throw new RuntimeException('The nonempty loop rendered its forelse branch.');
}
$empty = $renderer->render(view(__DIR__ . '/app/books.view.php', books: []));
if (!str_contains($empty, 'No books yet.')) {
    throw new RuntimeException('The empty loop did not render its forelse branch.');
}

$branches = $renderer->render(view(
    __DIR__ . '/app/branches.view.php',
    books: [new Book('Named'), new Book('')],
));
if (!str_contains($branches, '<p>Read Named</p>') || !str_contains($branches, '<p>Untitled book.</p>')) {
    throw new RuntimeException('A sibling else did not remain inside its foreach.');
}

$detail = $renderer->render(new BookView(new Book('Custom view')));
foreach (['<h1>Custom view</h1>', '<h2>Custom view</h2>', '<p>Read Custom view</p>'] as $expected) {
    if (!str_contains($detail, $expected)) {
        throw new RuntimeException("Missing custom View or slot content: {$expected}");
    }
}

// Public View properties are exposed through the renderer proxy; only
// explicitly supplied data is extracted into local template variables.
$probe = new BookView(new Book('Property'));
$probe->path = '<p>{{ isset($book) ? "unexpected local" : $this->book->title }}|{{ $selected->title }}</p>';
if ($renderer->render($probe) !== '<p>Property|Property</p>') {
    throw new RuntimeException('Custom View properties or data use the wrong scope.');
}

// An arbitrary caller local cannot leak into a component slot. Explicit
// attributes override global data, and each invocation has its own scope.
$scopes = $renderer->render(view(
    '<?php $outside = "caller"; ?>'
        . '<x-book :book="$selected"><x-slot name="heading">{{ $book->title }}</x-slot>'
        . '{{ $displayed->title }}|{{ $outside ?? "isolated" }}</x-book>',
    selected: new Book('Attribute'),
    book: new Book('Global'),
));
if (!str_contains($scopes, 'Attribute') || !str_contains($scopes, 'Attribute|isolated') || str_contains($scopes, 'Global')) {
    throw new RuntimeException('Component arguments and slot locals use the wrong scope.');
}

$configured = $renderer->render(new ConfiguredViews()->example());
foreach (['<h2>Configured views</h2>', '<aside>Configured views</aside>'] as $expected) {
    if (!str_contains($configured, $expected)) {
        throw new RuntimeException("Missing configured component or wrapper data: {$expected}");
    }
}

$dynamic = $renderer->render(new DynamicViews()->example());
foreach (['<h2>Dynamic views</h2>', '<span>2</span>', '<span>1</span>', '<p>Read Dynamic views</p>'] as $expected) {
    if (!str_contains($dynamic, $expected)) {
        throw new RuntimeException("Missing dynamic component or slot data: {$expected}");
    }
}
$dynamicTypes = $renderer->render(view(
    '<x-component is="x-dynamic-book" :book="$book" :count="2"><p>{{ get_debug_type($count) }}</p></x-component>',
    book: new Book('Types'),
));
if (!str_contains($dynamicTypes, '<p>string</p>')) {
    throw new RuntimeException('Dynamic scalar attributes must reach the selected component as strings.');
}

echo "Tempest view assertions passed.\n";
