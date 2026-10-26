<?php
/**
 * Symfony Demo Assertions
 *
 * Run: php examples/symfony/assertions.php
 *
 * These assertions load the demo's configuration with Symfony's own
 * loaders and check that every name the demo files declare, and every
 * claim their "Try:" comments make, holds at runtime. PHPantom reads the
 * same files statically, so a name that only it believes in is a bug in
 * the demo. Requires no kernel, database, or full application boot.
 */

require_once __DIR__ . '/vendor/autoload.php';

use App\Controller\DemoController;
use App\DependencyInjection\Configuration;
use App\Entity\User;
use App\EventListener\OrderPlacedListener;
use App\Form\RegistrationType;
use App\Message\CountWelcomeEmails;
use App\Message\SendWelcomeEmail;
use App\MessageHandler\CountWelcomeEmailsHandler;
use App\MessageHandler\SendWelcomeEmailHandler;
use App\Query\QueryBus;
use App\Service\WelcomeMailer;
use Doctrine\Bundle\DoctrineBundle\Repository\ServiceEntityRepository;
use Symfony\Bridge\Twig\Extension\RoutingExtension;
use Symfony\Bridge\Twig\Extension\TranslationExtension;
use Symfony\Bundle\FrameworkBundle\DependencyInjection\Configuration as FrameworkConfiguration;
use Symfony\Bundle\FrameworkBundle\Routing\AttributeRouteControllerLoader;
use Symfony\Component\Config\Definition\Processor;
use Symfony\Component\Config\FileLocator;
use Symfony\Component\Config\Loader\DelegatingLoader;
use Symfony\Component\Config\Loader\LoaderResolver;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\DependencyInjection\Loader\PhpFileLoader;
use Symfony\Component\DependencyInjection\Loader\YamlFileLoader as ServicesYamlLoader;
use Symfony\Component\EventDispatcher\Attribute\AsEventListener;
use Symfony\Component\Form\Forms;
use Symfony\Component\Messenger\Attribute\AsMessageHandler;
use Symfony\Component\Messenger\HandleTrait;
use Symfony\Component\Routing\Generator\UrlGenerator;
use Symfony\Component\Routing\Loader\AttributeDirectoryLoader;
use Symfony\Component\Routing\Loader\YamlFileLoader as RoutesYamlLoader;
use Symfony\Component\Routing\RequestContext;
use Symfony\Component\Translation\Loader\XliffFileLoader;
use Symfony\Component\Translation\Loader\YamlFileLoader as TranslationYamlLoader;
use Symfony\Component\Translation\Translator;
use Symfony\Component\Validator\Validation;
use Symfony\Component\Yaml\Yaml;
use Twig\Environment;
use Twig\Loader\FilesystemLoader;

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
 * @param ReflectionClass<object> $reflection
 * @param class-string $attribute
 * @return array<string, mixed>
 */
function attributeArguments(ReflectionClass $reflection, string $attribute): array
{
    $attributes = $reflection->getAttributes($attribute);
    return $attributes === [] ? [] : $attributes[0]->getArguments();
}

$config = __DIR__ . '/config';

// ─── Routes ─────────────────────────────────────────────────────────────────

// config/routes.yaml imports the controllers' #[Route] attributes and
// declares app_demo_legacy itself, as the template's path() calls claim.
$routeLocator = new FileLocator($config);
$routeLoader = new DelegatingLoader(new LoaderResolver([
    new RoutesYamlLoader($routeLocator),
    new AttributeDirectoryLoader($routeLocator, new AttributeRouteControllerLoader()),
]));
$routes = $routeLoader->load('routes.yaml');

check('app_demo is declared by the #[Route] on DemoController::index()', $routes->get('app_demo')?->getDefault('_controller') === DemoController::class . '::index');
check('app_demo_legacy is declared in config/routes.yaml', $routes->get('app_demo_legacy')?->getDefault('_controller') === DemoController::class . '::legacy');
check('The legacy route points at a method that exists', method_exists(DemoController::class, 'legacy'));

$urls = new UrlGenerator($routes, new RequestContext());
check("generateUrl('app_demo', ['userId' => ...]) builds its URL", $urls->generate('app_demo', ['userId' => 42]) === '/demo/42');

// ─── Services and parameters ────────────────────────────────────────────────

// Loaded, not compiled: compiling would autowire every class under src/,
// which needs the bundles' services the demo does not register.
$container = new ContainerBuilder();
$serviceLocator = new FileLocator($config);
(new ServicesYamlLoader($container, $serviceLocator))->load('services.yaml');
(new PhpFileLoader($container, $serviceLocator))->load('services.php');

check('app.welcome_mailer is declared in config/services.php', $container->hasDefinition('app.welcome_mailer'));
check('app.mailer is an alias of app.welcome_mailer', $container->hasAlias('app.mailer') && (string) $container->getAlias('app.mailer') === 'app.welcome_mailer');
check("app.legacy_mailer ('@app.welcome_mailer' in YAML) is an alias of app.welcome_mailer", $container->hasAlias('app.legacy_mailer') && (string) $container->getAlias('app.legacy_mailer') === 'app.welcome_mailer');
check('app.sender_name is a parameter declared in config/services.php', $container->getParameter('app.sender_name') === 'PHPantom');
check('app.support_email is a parameter declared in config/services.yaml', $container->getParameter('app.support_email') === 'support@example.com');
check('app.welcome_mailer, which app.mailer aliases, is built from WelcomeMailer', $container->getDefinition('app.welcome_mailer')->getClass() === WelcomeMailer::class);
check('app.support_email is a string', is_string($container->getParameter('app.support_email')));
check('The App\\ resource registers the controller as a service', $container->hasDefinition(DemoController::class));

// ─── Events ─────────────────────────────────────────────────────────────────

// DemoController dispatches app.order.placed; both subscriptions name it,
// and the method they name exists.
$listenerTags = $container->getDefinition(OrderPlacedListener::class)->getTag('kernel.event_listener');
check('The kernel.event_listener tag subscribes to app.order.placed', ($listenerTags[0]['event'] ?? null) === 'app.order.placed');
check("The tag's method exists", method_exists(OrderPlacedListener::class, $listenerTags[0]['method'] ?? ''));

$listener = attributeArguments(new ReflectionClass(OrderPlacedListener::class), AsEventListener::class);
check('#[AsEventListener] subscribes to app.order.placed', ($listener['event'] ?? null) === 'app.order.placed');
check("#[AsEventListener]'s method exists", method_exists(OrderPlacedListener::class, $listener['method'] ?? ''));

// ─── Messenger ──────────────────────────────────────────────────────────────

// FrameworkBundle's own schema accepts the bus configuration (it rejects a
// default_bus no bus declares).
$framework = (new Processor())->processConfiguration(
    new FrameworkConfiguration(false),
    [Yaml::parseFile("$config/packages/messenger.yaml")['framework']],
);
$buses = array_keys($framework['messenger']['buses']);
check('config/packages/messenger.yaml declares command.bus and query.bus', $buses === ['command.bus', 'query.bus']);

$handler = attributeArguments(new ReflectionClass(SendWelcomeEmailHandler::class), AsMessageHandler::class);
check('The handler names a bus messenger.yaml declares', in_array($handler['bus'] ?? null, $buses, true));
$handled = (new ReflectionMethod(SendWelcomeEmailHandler::class, '__invoke'))->getParameters()[0]->getType();
check('The handler receives SendWelcomeEmail', $handled instanceof ReflectionNamedType && $handled->getName() === SendWelcomeEmail::class);

// QueryBus::countWelcomeEmails() hands CountWelcomeEmails to HandleTrait,
// which returns what the message's one handler returns.
$queryHandler = attributeArguments(new ReflectionClass(CountWelcomeEmailsHandler::class), AsMessageHandler::class);
check('The query handler names a bus messenger.yaml declares', in_array($queryHandler['bus'] ?? null, $buses, true));
$queryMethod = new ReflectionMethod(CountWelcomeEmailsHandler::class, '__invoke');
$queried = $queryMethod->getParameters()[0]->getType();
check('The query handler receives CountWelcomeEmails', $queried instanceof ReflectionNamedType && $queried->getName() === CountWelcomeEmails::class);
check('The query handler returns an int', (string) $queryMethod->getReturnType() === 'int');
check('QueryBus uses HandleTrait', in_array(HandleTrait::class, class_uses(QueryBus::class), true));

// ─── Bundle configuration ───────────────────────────────────────────────────

// config/packages/acme_demo.yaml is valid under Configuration's schema.
$acme = (new Processor())->processConfiguration(
    new Configuration(),
    [Yaml::parseFile("$config/packages/acme_demo.yaml")['acme_demo']],
);
check('acme_demo.mailer.dsn is accepted by the schema', $acme['mailer']['dsn'] === 'smtp://localhost');
check('acme_demo.api_key is accepted by the schema', $acme['api_key'] === 'local-development-key');

// ─── Translations ───────────────────────────────────────────────────────────

$translator = new Translator('en');
$translator->addLoader('yaml', new TranslationYamlLoader());
$translator->addLoader('xlf', new XliffFileLoader());
$translator->addResource('yaml', __DIR__ . '/translations/messages.en.yaml', 'en', 'messages');
$translator->addResource('xlf', __DIR__ . '/translations/validators.en.xlf', 'en', 'validators');

check('demo.title is declared in the messages domain', $translator->trans('demo.title') === 'Symfony intelligence');
check('demo.card is declared in the messages domain', $translator->trans('demo.card') === 'Every string links back to its declaration');
check('user.invalid_email is declared in the validators domain', $translator->trans('user.invalid_email', [], 'validators') === 'Please enter a valid email address.');
check('user.invalid_email is not in the messages domain', $translator->trans('user.invalid_email') === 'user.invalid_email');

// ─── Templates ──────────────────────────────────────────────────────────────

// Rendering demo/index.html.twig resolves its extends, include, path(), and
// trans filter for real.
$twig = new Environment(new FilesystemLoader(__DIR__ . '/templates'), ['strict_variables' => true]);
$twig->addExtension(new TranslationExtension($translator));
$twig->addExtension(new RoutingExtension($urls));
$html = $twig->render('demo/index.html.twig', ['sender' => 'PHPantom', 'support' => '', 'title' => '']);

check('demo/index.html.twig extends base.html.twig', str_contains($html, '<!doctype html>'));
check('demo/index.html.twig includes demo/_card.html.twig', str_contains($html, 'Every string links back to its declaration'));
check("The template's 'demo.title'|trans is translated", str_contains($html, 'Symfony intelligence'));
check("The template's path('app_demo', ...) resolves", str_contains($html, 'href="/demo/42"'));
check("The template's path('app_demo_legacy') resolves", str_contains($html, 'href="/legacy"'));

// ─── Forms ──────────────────────────────────────────────────────────────────

// Each field RegistrationType adds reads and writes the data-class property
// of the same name.
$user = new User('ada@example.com', 'Ada');
$form = Forms::createFormFactory()->create(RegistrationType::class, $user);
check("The 'email' field reads User's email", $form->get('email')->getData() === 'ada@example.com');
check("The 'displayName' field reads User's displayName", $form->get('displayName')->getData() === 'Ada');
$form->submit(['email' => 'grace@example.com', 'displayName' => 'Grace']);
check("The 'email' field writes User's email", $user->getEmail() === 'grace@example.com');
check("The 'displayName' field writes User's displayName", $user->getDisplayName() === 'Grace');

// ─── Validation ─────────────────────────────────────────────────────────────

// config/validator/User.yaml maps constraints onto properties User declares.
$validator = Validation::createValidatorBuilder()
    ->addYamlMapping("$config/validator/User.yaml")
    ->getValidator();
$violations = $validator->validate(new User('', 'A'));
$paths = [];
foreach ($violations as $violation) {
    $paths[] = $violation->getPropertyPath();
}
sort($paths);
check('NotBlank on email and Length on displayName both apply', $paths === ['displayName', 'email']);
check('A valid user has no violations', count($validator->validate(new User('ada@example.com', 'Ada'))) === 0);

// ─── Doctrine mapping ───────────────────────────────────────────────────────

$mapping = simplexml_load_file("$config/doctrine/User.orm.xml");
$entity = (string) $mapping->entity['name'];
$repository = (string) $mapping->entity['repository-class'];
check('The XML mapping names the User entity', $entity === User::class);
check('The XML mapping names a ServiceEntityRepository', is_subclass_of($repository, ServiceEntityRepository::class));

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
