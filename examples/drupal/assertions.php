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
use Drupal\acme\Entity\Note;
use Drupal\acme\Greeter;
use Drupal\acme\Hook\AcmeHooks;
use Drupal\Component\FileCache\FileCacheFactory;
use Drupal\Component\Serialization\Yaml;
use Drupal\Core\Controller\ControllerBase;
use Drupal\Core\DependencyInjection\ContainerBuilder;
use Drupal\Core\DependencyInjection\YamlFileLoader;
use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Extension\InfoParser;
use Drupal\Core\Hook\Attribute\Hook;

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

// The demo's classes live in the module, which only a booted site adds to
// the autoloader.
spl_autoload_register(static function (string $class) use ($module): void {
    if (str_starts_with($class, 'Drupal\\acme\\')) {
        $file = "$module/src/" . str_replace('\\', '/', substr($class, strlen('Drupal\\acme\\'))) . '.php';
        if (is_file($file)) {
            require $file;
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
check('The Greeter class name is an alias of acme.greeter', $container->hasAlias(Greeter::class) && (string) $container->getAlias(Greeter::class) === 'acme.greeter');
check("acme.greeter receives core's config.factory", (string) $container->getDefinition('acme.greeter')->getArgument(0) === 'config.factory');

// ─── Routes ─────────────────────────────────────────────────────────────────

$routes = Yaml::decode((string) file_get_contents("$module/acme.routing.yml"));
[$controllerClass, $controllerMethod] = explode('::', ltrim($routes['acme.greeting']['defaults']['_controller'] ?? '', '\\')) + [1 => ''];

check('acme.greeting is declared in acme.routing.yml', isset($routes['acme.greeting']));
check('acme.greeting points at GreetingController::greet()', $controllerClass === GreetingController::class && method_exists($controllerClass, $controllerMethod));
check('acme.greeting declares the {name} the controller takes', str_contains($routes['acme.greeting']['path'], '{name}') && (new ReflectionMethod(GreetingController::class, 'greet'))->getParameters()[0]->getName() === 'name');
check('GreetingController inherits the helpers of ControllerBase', is_subclass_of(GreetingController::class, ControllerBase::class));

// ─── Entity types ───────────────────────────────────────────────────────────

$note = new ReflectionClass(Note::class);
$noteType = attributeArguments($note, ContentEntityType::class);
check('Note declares the acme_note entity type by attribute', ($noteType['id'] ?? null) === 'acme_note');
check("acme_note's label key is the title base field", ($noteType['entity_keys']['label'] ?? null) === 'title');
check('Note is a content entity', $note->isSubclassOf(ContentEntityBase::class));
check('Note declares its own base fields', $note->getMethod('baseFieldDefinitions')->getDeclaringClass()->getName() === Note::class);

// ─── Hooks and theme hooks ──────────────────────────────────────────────────

require_once "$module/acme.module";
require_once "$theme/acme_theme.theme";

$themeHook = new ReflectionMethod(AcmeHooks::class, 'theme');
$themeHooks = (new AcmeHooks())->theme();
check('acme_help() implements hook_help procedurally', function_exists('acme_help'));
check("AcmeHooks::theme() implements hook_theme through #[Hook('theme')]", attributeArguments($themeHook, Hook::class) === ['theme']);
check('hook_theme declares the acme_greeting theme hook', isset($themeHooks['acme_greeting']));
check('acme_greeting declares the variables the controller passes', array_keys($themeHooks['acme_greeting']['variables'] ?? []) === ['name', 'greeting']);
check('acme_greeting has the template its name implies', is_file("$module/templates/acme-greeting.html.twig"));
check('acme_theme preprocesses acme_greeting', function_exists('acme_theme_preprocess_acme_greeting'));

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
