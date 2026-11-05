<?php
/**
 * Drupal Demo Assertions
 *
 * Run: php examples/drupal/assertions.php
 *
 * These assertions read the demo's declarations with Drupal's own loaders
 * and with reflection, and check that every name the demo files declare,
 * and every claim their "Try:" comments make, holds at runtime. PHPantom
 * reads the same files statically, so a name that only it believes in is a
 * bug in the demo. Requires no kernel, database, or site install.
 */

require_once __DIR__ . '/vendor/autoload.php';

use Drupal\acme\Controller\GreetingController;
use Drupal\acme\Controller\NoteController;
use Drupal\acme\Entity\Note;
use Drupal\acme\Greeter;
use Drupal\acme\Hook\AcmeHooks;
use Drupal\acme\NoteCounter;
use Drupal\Component\FileCache\FileCacheFactory;
use Drupal\Component\Serialization\Yaml;
use Drupal\Core\Controller\ControllerBase;
use Drupal\Core\DependencyInjection\ContainerBuilder;
use Drupal\Core\DependencyInjection\ContainerInjectionInterface;
use Drupal\Core\DependencyInjection\YamlFileLoader;
use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\ContentEntityInterface;
use Drupal\Core\Entity\ContentEntityTypeInterface;
use Drupal\Core\Entity\EntityInterface;
use Drupal\Core\Entity\EntityTypeManager;
use Drupal\Core\Entity\Sql\SqlContentEntityStorage;
use Drupal\Core\Extension\InfoParser;
use Drupal\Core\Hook\Attribute\Hook;
use Drupal\Core\Language\LanguageManager;
use Drupal\Core\Link;
use Drupal\Core\Url;
use Drupal\language\ConfigurableLanguageManager;
use Drupal\language\LanguageServiceProvider;

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
 * The arguments of the first `$attribute` on `$reflection`.
 *
 * @param ReflectionClass<object>|ReflectionMethod $reflection
 * @param class-string $attribute
 * @return array<int|string, mixed>
 */
function attributeArguments(ReflectionClass|ReflectionMethod $reflection, string $attribute): array
{
    $attributes = $reflection->getAttributes($attribute);
    return $attributes === [] ? [] : $attributes[0]->getArguments();
}

$web = __DIR__ . '/web';
$module = "$web/modules/custom/acme";
$theme = "$web/themes/custom/acme_theme";

// The demo's classes live in the module, and the language module's in
// core, which only a booted site adds to the autoloader.
spl_autoload_register(static function (string $class) use ($module, $web): void {
    foreach (['acme' => $module, 'language' => "$web/core/modules/language"] as $name => $dir) {
        $prefix = "Drupal\\$name\\";
        if (str_starts_with($class, $prefix)) {
            $file = "$dir/src/" . str_replace('\\', '/', substr($class, strlen($prefix))) . '.php';
            if (is_file($file)) {
                require $file;
            }
        }
    }
});

// ─── Extensions ─────────────────────────────────────────────────────────────

$infoParser = new InfoParser($web);
$moduleInfo = $infoParser->parse("$module/acme.info.yml");
$themeInfo = $infoParser->parse("$theme/acme_theme.info.yml");
check('acme is a module this core accepts', $moduleInfo['type'] === 'module');
check('acme_theme is a theme this core accepts', $themeInfo['type'] === 'theme');

// ─── Services ───────────────────────────────────────────────────────────────

// Loaded, not compiled: compiling would need core's own services. The
// loader caches parsed files, which a booted site configures.
FileCacheFactory::setPrefix('phpantom-demo');
FileCacheFactory::setConfiguration([FileCacheFactory::DISABLE_CACHE => true]);
$container = new ContainerBuilder();
(new YamlFileLoader($container))->load("$module/acme.services.yml");

check('acme.greeter is declared in acme.services.yml', $container->hasDefinition('acme.greeter'));
check('acme.greeter is built from Greeter', $container->getDefinition('acme.greeter')->getClass() === Greeter::class);
check('The class acme.services.yml names exists', class_exists((string) $container->getDefinition('acme.greeter')->getClass()));
check('The Greeter class name is an alias of acme.greeter', $container->hasAlias(Greeter::class) && (string) $container->getAlias(Greeter::class) === 'acme.greeter');
check("acme.greeter receives core's config.factory", (string) $container->getDefinition('acme.greeter')->getArgument(0) === 'config.factory');
check('acme.greeter receives the acme.greeting_format parameter', $container->getDefinition('acme.greeter')->getArgument(1) === '%acme.greeting_format%');
check('acme.greeting_format is a parameter declared in acme.services.yml', is_string($container->getParameter('acme.greeting_format')));
check('Greeter takes the format as its second argument', (new ReflectionMethod(Greeter::class, '__construct'))->getParameters()[1]->getType()?->getName() === 'string');

// Core declares the services the module looks up by ID.
$core = new ContainerBuilder();
(new YamlFileLoader($core))->load("$web/core/core.services.yml");
check('core.services.yml declares config.factory', $core->hasDefinition('config.factory'));
check('entity_type.manager, which NoteCounter::create() looks up, is built from EntityTypeManager', $core->getDefinition('entity_type.manager')->getClass() === EntityTypeManager::class);
check('NoteCounter takes what entity_type.manager is', is_subclass_of(EntityTypeManager::class, (string) (new ReflectionMethod(NoteCounter::class, '__construct'))->getParameters()[0]->getType()));
check('NoteCounter is built from the container', is_subclass_of(NoteCounter::class, ContainerInjectionInterface::class));

// The language module's service provider builds core's language_manager
// from its own class. Its default language values come from the site's
// configuration, which there is none of here.
$altered = new ContainerBuilder();
(new YamlFileLoader($altered))->load("$web/core/core.services.yml");
$languageProvider = new class extends LanguageServiceProvider {
    protected function getDefaultLanguageValues()
    {
        return false;
    }
};
check('core.services.yml builds language_manager from LanguageManager', $altered->getDefinition('language_manager')->getClass() === LanguageManager::class);
check('Drupal looks for the language module\'s provider under the name LanguageServiceProvider', ContainerBuilder::camelize('language') . 'ServiceProvider' === (new ReflectionClass(LanguageServiceProvider::class))->getShortName());
$languageProvider->alter($altered);
check('LanguageServiceProvider::alter() builds language_manager from ConfigurableLanguageManager', ltrim((string) $altered->getDefinition('language_manager')->getClass(), '\\') === ConfigurableLanguageManager::class);
check('Only ConfigurableLanguageManager declares getLanguageConfigOverride()', method_exists(ConfigurableLanguageManager::class, 'getLanguageConfigOverride') && !method_exists(LanguageManager::class, 'getLanguageConfigOverride'));
check('acme_french_site_name() looks up language_manager', str_contains((string) file_get_contents("$module/acme.module"), "\\Drupal::service('language_manager')"));

// ─── Routes ─────────────────────────────────────────────────────────────────

$routes = Yaml::decode((string) file_get_contents("$module/acme.routing.yml"));
[$controllerClass, $controllerMethod] = explode('::', ltrim($routes['acme.greeting']['defaults']['_controller'] ?? '', '\\')) + [1 => ''];

check('acme.greeting is declared in acme.routing.yml', isset($routes['acme.greeting']));
check('acme.greeting points at GreetingController::greet()', $controllerClass === GreetingController::class && method_exists($controllerClass, $controllerMethod));
check('The _controller string names a public method', (new ReflectionMethod($controllerClass, $controllerMethod))->isPublic());
check('acme.greeting declares the {name} the controller takes', str_contains($routes['acme.greeting']['path'], '{name}') && (new ReflectionMethod(GreetingController::class, 'greet'))->getParameters()[0]->getName() === 'name');
check('GreetingController inherits the helpers of ControllerBase', is_subclass_of(GreetingController::class, ControllerBase::class));
check('ControllerBase::redirect(), which greetTheWorld() calls, takes a route name first', (new ReflectionMethod(ControllerBase::class, 'redirect'))->getParameters()[0]->getName() === 'route_name');
check('Url::fromRoute() takes a route name first', (new ReflectionMethod(Url::class, 'fromRoute'))->getParameters()[0]->getName() === 'route_name');
check('Link::createFromRoute() takes a route name second', (new ReflectionMethod(Link::class, 'createFromRoute'))->getParameters()[1]->getName() === 'route_name');

// Every route the demo names is one the routing file declares.
$controllerSource = (string) file_get_contents("$module/src/Controller/GreetingController.php");
preg_match_all("/(?:redirect|fromRoute)\\('([^']+)'|createFromRoute\\([^,]+, '([^']+)'/", $controllerSource, $named);
$namedInPhp = array_values(array_filter(array_merge($named[1], $named[2])));
check('GreetingController names routes in redirect(), fromRoute() and createFromRoute()', count($namedInPhp) === 3);
check('every route GreetingController names is declared', array_diff($namedInPhp, array_keys($routes)) === []);
$menuLinks = Yaml::decode((string) file_get_contents("$module/acme.links.menu.yml"));
check('the acme.greeting menu link points at a declared route', isset($routes[$menuLinks['acme.greeting']['route_name'] ?? '']));
check("the menu link's parameters are the ones the route's path declares", array_keys($menuLinks['acme.greeting']['route_parameters'] ?? []) === ['name']);
preg_match_all("/path\\('([^']+)'/", (string) file_get_contents("$module/templates/acme-greeting.html.twig"), $namedInTwig);
check('the acme_greeting template links to a declared route', $namedInTwig[1] !== [] && array_diff($namedInTwig[1], array_keys($routes)) === []);
check("the template passes the name parameter acme.greeting's path declares", str_contains((string) file_get_contents("$module/templates/acme-greeting.html.twig"), "path('acme.greeting', {'name': 'World'})"));
check('worldLink() passes the name parameter the path declares', str_contains($controllerSource, "Url::fromRoute('acme.greeting', ['name' => 'World'])"));

// The routes CoreLinks names are built by core, not declared in a routing file.
$coreLinksSource = (string) file_get_contents("$module/src/CoreLinks.php");
$nodeEntitySource = (string) file_get_contents("$web/core/modules/node/src/Entity/Node.php");
$nodeRoutesSource = (string) file_get_contents("$web/core/modules/node/src/Entity/NodeRouteProvider.php");
$entityRoutesSource = (string) file_get_contents("$web/core/lib/Drupal/Core/Entity/Routing/DefaultHtmlRouteProvider.php");
$contentView = Yaml::decode((string) file_get_contents("$web/core/modules/node/config/optional/views.view.content.yml"));
check('CoreLinks passes entity.node.canonical its node parameter', str_contains($coreLinksSource, "Url::fromRoute('entity.node.canonical', ['node' => 1])"));
check("DefaultHtmlRouteProvider builds entity.<type>.canonical from the entity type's ID", str_contains($entityRoutesSource, '$collection->add("entity.{$entity_type_id}.canonical", $canonical_route);'));
check("node's html route provider is NodeRouteProvider, a DefaultHtmlRouteProvider", str_contains($nodeEntitySource, "'html' => NodeRouteProvider::class") && str_contains($nodeRoutesSource, 'class NodeRouteProvider extends DefaultHtmlRouteProvider'));
check("node's canonical link takes the {node} parameter", str_contains($nodeEntitySource, "'canonical' => '/node/{node}'"));
check('CoreLinks names view.content.page_1', str_contains($coreLinksSource, "Url::fromRoute('view.content.page_1')"));
check('node ships the content view with the page_1 page display', ($contentView['id'] ?? '') === 'content' && ($contentView['display']['page_1']['display_plugin'] ?? '') === 'page');

// ─── Entity types ───────────────────────────────────────────────────────────

$note = new ReflectionClass(Note::class);
$noteType = attributeArguments($note, ContentEntityType::class);
check('Note declares the acme_note entity type by attribute', ($noteType['id'] ?? null) === 'acme_note');
check("acme_note's label key is the title base field", ($noteType['entity_keys']['label'] ?? null) === 'title');
check('Note is a content entity', $note->isSubclassOf(ContentEntityBase::class));
check('Note declares its own base fields', $note->getMethod('baseFieldDefinitions')->getDeclaringClass()->getName() === Note::class);
// Attribute discovery tells the attribute which class it is on.
$noteAttribute = $note->getAttributes(ContentEntityType::class)[0]->newInstance();
$noteAttribute->setClass(Note::class);
$noteDefinition = $noteAttribute->get();
check("acme_note's storage, which NoteCounter::first() loads from, is core's SQL storage", $noteDefinition->getStorageClass() === SqlContentEntityStorage::class);
check('acme_note loads Note', $noteDefinition->getClass() === Note::class);
check("NoteCounter::ids() starts an entity query on acme_note", str_contains((string) file_get_contents("$module/src/NoteCounter.php"), "\\Drupal::entityQuery('acme_note')"));
check('acme_note is a content entity type, whose entity query gives IDs under integer keys', $noteDefinition instanceof ContentEntityTypeInterface && $noteDefinition->entityClassImplements(ContentEntityInterface::class));
check('NoteCounter::count() counts the entity query it starts from the storage', str_contains((string) file_get_contents("$module/src/NoteCounter.php"), "->getQuery()") && str_contains((string) file_get_contents("$module/src/NoteCounter.php"), "->count()"));
check('NoteCounter::first() asks for acme_note', str_contains((string) file_get_contents("$module/src/NoteCounter.php"), "getStorage('acme_note');"));

// Drupal upcasts a controller's parameter named after an entity type and
// hinted with a class or interface the entity class is (core's
// EntityResolverManager::setParametersFromReflection()).
[$noteControllerClass, $noteControllerMethod] = explode('::', ltrim($routes['acme.note']['defaults']['_controller'] ?? '', '\\')) + [1 => ''];
$noteView = new ReflectionMethod(NoteController::class, 'view');
$noteParameter = $noteView->getParameters()[0];
check('acme.note points at NoteController::view()', $noteControllerClass === NoteController::class && $noteControllerMethod === 'view');
check('acme.note declares the {acme_note} parameter', str_contains($routes['acme.note']['path'], '{acme_note}'));
check('NoteController::view() names its parameter after the acme_note entity type', $noteParameter->getName() === ($noteType['id'] ?? null));
check('NoteController::view() hints the parameter with an interface Note implements, so Drupal upcasts it', $noteParameter->getType()?->getName() === EntityInterface::class && is_subclass_of(Note::class, EntityInterface::class));
check("acme.note's title callback is NoteController::title(), which receives the same parameter", ltrim($routes['acme.note']['defaults']['_title_callback'] ?? '', '\\') === NoteController::class . '::title' && (new ReflectionMethod(NoteController::class, 'title'))->getParameters()[0]->getName() === 'acme_note');

// ─── Hooks and theme hooks ──────────────────────────────────────────────────

require_once "$module/acme.module";
require_once "$theme/acme_theme.theme";

$themeHook = new ReflectionMethod(AcmeHooks::class, 'theme');
$themeHooks = (new AcmeHooks())->theme();
check('acme_help() implements hook_help procedurally', function_exists('acme_help'));
check('acme_help() also looks the greeter up by its class name, which acme.services.yml registers', str_contains((string) file_get_contents("$module/acme.module"), '\\Drupal::service(Greeter::class)') && $container->hasAlias(Greeter::class));
check("acme_help() looks up acme.greeter, which acme.services.yml declares", str_contains((string) file_get_contents("$module/acme.module"), "\\Drupal::service('acme.greeter')") && $container->hasDefinition('acme.greeter'));
check("AcmeHooks::theme() implements hook_theme through #[Hook('theme')]", attributeArguments($themeHook, Hook::class) === ['theme']);
check('hook_theme declares the acme_greeting theme hook', isset($themeHooks['acme_greeting']));
check('acme_greeting declares the variables the controller passes', array_keys($themeHooks['acme_greeting']['variables'] ?? []) === ['name', 'greeting']);
check('acme_greeting has the template its name implies', is_file("$module/templates/acme-greeting.html.twig"));
check('acme_theme preprocesses acme_greeting', function_exists('acme_theme_preprocess_acme_greeting'));

// The module documents, invokes, and implements its own hook, and
// implements core's form alter hooks.
require_once "$module/acme.api.php";
$greetingAlter = new ReflectionMethod(AcmeHooks::class, 'greetingAlter');
check('acme.api.php documents hook_acme_greeting_alter()', function_exists('hook_acme_greeting_alter'));
check('Greeter invokes the acme_greeting alter hook', str_contains((string) file_get_contents("$module/src/Greeter.php"), "->alter('acme_greeting', \$greeting, \$name)"));
check("AcmeHooks::greetingAlter() implements it through #[Hook('acme_greeting_alter')]", attributeArguments($greetingAlter, Hook::class) === ['acme_greeting_alter']);
check("greetingAlter()'s untyped name is the documented string", $greetingAlter->getParameters()[1]->getType() === null && (string) (new ReflectionFunction('hook_acme_greeting_alter'))->getParameters()[1]->getType() === 'string');
check('acme_form_alter() implements hook_form_alter procedurally', function_exists('acme_form_alter'));
check("core documents hook_form_alter() with the FormStateInterface acme_form_alter() leaves untyped", str_contains((string) file_get_contents("$web/core/lib/Drupal/Core/Form/form.api.php"), 'function hook_form_alter(&$form, \\Drupal\\Core\\Form\\FormStateInterface $form_state, $form_id)') && (new ReflectionFunction('acme_form_alter'))->getParameters()[1]->getType() === null);
check("core documents hook_form_alter() with a docblock acme_form_alter()'s hover shows", (bool) preg_match('~/\*\*\n \* Perform alterations before a form is rendered\.\n(?: \*[^\n]*\n)* \*/\nfunction hook_form_alter\(~', (string) file_get_contents("$web/core/lib/Drupal/Core/Form/form.api.php")));
check("core's FormBuilder::prepareForm() fires the form alter hooks", str_contains((string) file_get_contents("$web/core/lib/Drupal/Core/Form/FormBuilder.php"), '$this->moduleHandler->alter($hooks, $form, $form_state, $form_id);'));

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
