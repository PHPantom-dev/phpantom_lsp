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

use App\Admin\DashboardController;
use App\Controller\DemoController;
use App\DependencyInjection\Configuration;
use App\Entity\Team;
use App\Entity\User;
use App\DependencyInjection\AcmeDemoExtension;
use App\EventListener\OrderPlacedListener;
use App\Form\RegistrationType;
use App\Message\CountWelcomeEmails;
use App\Message\SendWelcomeEmail;
use App\MessageHandler\CountWelcomeEmailsHandler;
use App\MessageHandler\SendWelcomeEmailHandler;
use App\Query\QueryBus;
use App\Service\CachingSlugger;
use App\Service\Slugger;
use App\Service\WelcomeMailer;
use App\Twig\AppExtension;
use App\Twig\Badges;
use Doctrine\Bundle\DoctrineBundle\Repository\ServiceEntityRepository;
use Doctrine\DBAL\DriverManager;
use Doctrine\ORM\EntityManager;
use Doctrine\ORM\Mapping\ClassMetadata;
use Doctrine\ORM\Mapping\Driver\SimplifiedXmlDriver;
use Doctrine\ORM\ORMSetup;
use Doctrine\ORM\Query\QueryException;
use Doctrine\ORM\Tools\SchemaTool;
use Symfony\Bridge\Twig\Extension\RoutingExtension;
use Symfony\Bridge\Twig\Extension\TranslationExtension;
use Symfony\Bundle\FrameworkBundle\DependencyInjection\Configuration as FrameworkConfiguration;
use Symfony\Bundle\FrameworkBundle\FrameworkBundle;
use Symfony\Bundle\FrameworkBundle\Routing\AttributeRouteControllerLoader;
use Symfony\Component\Config\Definition\Processor;
use Symfony\Component\Config\FileLocator;
use Symfony\Component\Config\Loader\DelegatingLoader;
use Symfony\Component\Config\Loader\LoaderResolver;
use Symfony\Component\DependencyInjection\ChildDefinition;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\DependencyInjection\Loader\PhpFileLoader;
use Symfony\Component\DependencyInjection\Loader\YamlFileLoader as ServicesYamlLoader;
use Symfony\Component\EventDispatcher\Attribute\AsEventListener;
use Symfony\Component\Form\Extension\Core\Type\EmailType;
use Symfony\Component\Form\Extension\Validator\ValidatorExtension;
use Symfony\Component\Form\Forms;
use Symfony\Component\Messenger\Attribute\AsMessageHandler;
use Symfony\Component\Messenger\HandleTrait;
use Symfony\Component\Routing\Generator\UrlGenerator;
use Symfony\Component\Routing\Loader\AttributeDirectoryLoader;
use Symfony\Component\Routing\Loader\PhpFileLoader as RoutesPhpLoader;
use Symfony\Component\Routing\Loader\YamlFileLoader as RoutesYamlLoader;
use Symfony\Component\Routing\RequestContext;
use Symfony\Component\Serializer\Mapping\Factory\ClassMetadataFactory;
use Symfony\Component\Serializer\Mapping\Loader\AttributeLoader;
use Symfony\Component\Serializer\Normalizer\ObjectNormalizer;
use Symfony\Component\Serializer\Serializer;
use Symfony\Component\Translation\Loader\XliffFileLoader;
use Symfony\Component\Translation\Loader\YamlFileLoader as TranslationYamlLoader;
use Symfony\Component\Translation\Translator;
use Symfony\Component\Validator\Constraints\Length;
use Symfony\Component\Validator\Validation;
use Symfony\Component\Yaml\Yaml;
use Twig\Environment;
use Twig\Extension\AttributeExtension;
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
check('app_demo_profile is declared by the #[Route] on DemoController::profile()', $routes->get('app_demo_profile')?->getDefault('_controller') === DemoController::class . '::profile');

check("The admin import's name_prefix names DashboardController's route admin_dashboard", $routes->get('admin_dashboard')?->getDefault('_controller') === DashboardController::class . '::index');
check("'dashboard' on its own is not a route", $routes->get('dashboard') === null);

// config/routes/framework.yaml imports FrameworkBundle's own routing file,
// which declares _preview_error. The bundle's path is its class's directory.
$errorsResource = Yaml::parseFile("$config/routes/framework.yaml")['when@dev']['_errors']['resource'];
$frameworkBundleDir = dirname((new ReflectionClass(FrameworkBundle::class))->getFileName());
$errorRoutes = (new RoutesPhpLoader(new FileLocator()))->load(str_replace('@FrameworkBundle', $frameworkBundleDir, $errorsResource));
check("FrameworkBundle's errors.php declares _preview_error", $errorRoutes->get('_preview_error') !== null);
$errorRoutes->addPrefix('/_error');
$routes->addCollection($errorRoutes);

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
check("app.greeting's %app.sender_name% placeholder reads app.sender_name", $container->getParameterBag()->resolveValue($container->getParameter('app.greeting')) === 'Hello from PHPantom');
$mailerCalls = $container->getDefinition(WelcomeMailer::class)->getMethodCalls();
check("WelcomeMailer's calls: in config/services.yaml calls setSender", ($mailerCalls[0][0] ?? null) === 'setSender' && method_exists(WelcomeMailer::class, 'setSender'));
$demoCache = $container->getDefinition('app.demo_cache');
check('app.demo_cache is a child of cache.app', $demoCache instanceof ChildDefinition && $demoCache->getParent() === 'cache.app');
check('app.slugger is built from Slugger', $container->getDefinition('app.slugger')->getClass() === Slugger::class);
check('CachingSlugger decorates app.slugger', ($container->getDefinition(CachingSlugger::class)->getDecoratedService()[0] ?? null) === 'app.slugger');
check('Only CachingSlugger declares cachedCount()', method_exists(CachingSlugger::class, 'cachedCount') && !method_exists(Slugger::class, 'cachedCount'));
$cachingSlugger = new CachingSlugger(new Slugger());
check('CachingSlugger slugs as Slugger does, and counts what it cached', $cachingSlugger->slug('Hello World') === 'hello-world' && $cachingSlugger->cachedCount() === 1);

// FrameworkBundle declares cache.app in its own configuration, read from
// the bundle's directory.
$frameworkServices = new ContainerBuilder();
(new PhpFileLoader($frameworkServices, new FileLocator("$frameworkBundleDir/Resources/config")))->load('cache.php');
check("FrameworkBundle's Resources/config/cache.php declares cache.app", $frameworkServices->hasDefinition('cache.app'));

// The demo's own extension declares services through the container builder.
(new AcmeDemoExtension())->load([Yaml::parseFile("$config/packages/acme_demo.yaml")['acme_demo']], $container);
check('AcmeDemoExtension registers app.dsn_mailer from WelcomeMailer', $container->getDefinition('app.dsn_mailer')->getClass() === WelcomeMailer::class);
check('app.default_mailer is an alias of app.dsn_mailer', (string) $container->getAlias('app.default_mailer') === 'app.dsn_mailer');
check('AcmeDemoExtension sets app.mailer_dsn from the bundle configuration', $container->getParameter('app.mailer_dsn') === 'smtp://localhost');

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
check('A message no catalogue declares is its own translation', $translator->trans('Save') === 'Save');
check('symfony/validator ships a catalogue of the validators domain', is_file(__DIR__ . '/vendor/symfony/validator/Resources/translations/validators.en.xlf'));

// ─── Templates ──────────────────────────────────────────────────────────────

// Rendering demo/index.html.twig resolves its extends, include, path(), and
// trans filter for real.
$twig = new Environment(new FilesystemLoader(__DIR__ . '/templates'), ['strict_variables' => true]);
$twig->addExtension(new TranslationExtension($translator));
$twig->addExtension(new RoutingExtension($urls));
$twig->addExtension(new AppExtension());
$twig->addExtension(new AttributeExtension(Badges::class));
foreach (Yaml::parseFile("$config/packages/twig.yaml")['twig']['globals'] as $name => $value) {
    $twig->addGlobal($name, $value);
}
$html = $twig->render('demo/index.html.twig', [
    'sender' => 'PHPantom',
    'support' => '',
    'title' => '',
    'user' => new User('ada@example.com', 'Ada'),
]);

check('demo/index.html.twig extends base.html.twig', str_contains($html, '<!doctype html>'));
check("demo/index.html.twig's title block overrides base.html.twig's, and parent() renders it", str_contains($html, '<title>Demo · Symfony demo</title>'));
check('demo/index.html.twig includes demo/_card.html.twig', str_contains($html, 'Every string links back to its declaration'));
check("The template's 'demo.title'|trans is translated", str_contains($html, 'Symfony intelligence'));
check("The template's path('app_demo', ...) resolves", str_contains($html, 'href="/demo/42"'));
check("The template's path('app_demo_legacy') resolves", str_contains($html, 'href="/legacy"'));
check("The template's path('admin_dashboard') resolves", str_contains($html, 'href="/admin"'));
check("The template's path('_preview_error', ...) resolves", str_contains($html, 'href="/_error/404"'));
check("The template's |initials filter is AppExtension::initials()", str_contains($html, '<p>P <span class="badge">demo</span> PHPANTOM</p>'));
check('demo/_card.html.twig is handed the sender demo/index.html.twig is rendered with', str_contains($html, 'Every string links back to its declaration, PHPantom'));
check('config/packages/twig.yaml declares support_url as a global', str_contains($html, 'href="https://example.com/support"'));
check("The template's user.displayName and user.email read User's getters", str_contains($html, '<p>Ada &lt;ada@example.com&gt;</p>'));

// ─── Forms ──────────────────────────────────────────────────────────────────

// Each field RegistrationType adds reads and writes the data-class property
// of the same name.
$user = new User('ada@example.com', 'Ada');
// The validator's type extension gives every form its 'constraints' option.
$formFactory = Forms::createFormFactoryBuilder()
    ->addExtension(new ValidatorExtension(Validation::createValidator()))
    ->getFormFactory();
$form = $formFactory->create(RegistrationType::class, $user);
check("The 'email' field reads User's email", $form->get('email')->getData() === 'ada@example.com');
check("The 'displayName' field reads User's displayName", $form->get('displayName')->getData() === 'Ada');
$form->submit(['email' => 'grace@example.com', 'displayName' => 'Grace']);
check("The 'email' field writes User's email", $user->getEmail() === 'grace@example.com');
check("The 'displayName' field writes User's displayName", $user->getDisplayName() === 'Grace');

// The 'email' field's options are EmailType's, its parents', and the
// validator extension's, and Length takes max as a constructor parameter.
$emailConfig = $form->get('email')->getConfig();
check("The 'email' field is an EmailType", $emailConfig->getType()->getInnerType() instanceof EmailType);
check("'help' is an option the 'email' field takes", $emailConfig->getOption('help') === 'We never share it.');
$constraints = $emailConfig->getOption('constraints');
check("'constraints' holds Length(max: 180)", $constraints[0] instanceof Length && $constraints[0]->max === 180);
$threwOnUnknownOption = false;
try {
    $formFactory->createBuilder(EmailType::class, null, ['hlep' => 'x']);
} catch (\Symfony\Component\OptionsResolver\Exception\UndefinedOptionsException) {
    $threwOnUnknownOption = true;
}
check('An option no type declares throws, as PHPantom reports it', $threwOnUnknownOption);

// ─── Serializer groups ──────────────────────────────────────────────────────

// The #[Groups] on User's getters put them in 'user:read' and 'user:list',
// which DemoController::profile() serializes with.
$serializer = new Serializer([new ObjectNormalizer(new ClassMetadataFactory(new AttributeLoader()))]);
$ada = new User('ada@example.com', 'Ada');
check("'user:list' holds displayName only", $serializer->normalize($ada, null, ['groups' => ['user:list']]) === ['displayName' => 'Ada']);
check("'user:read' holds email and displayName", $serializer->normalize($ada, null, ['groups' => ['user:read']]) === ['email' => 'ada@example.com', 'displayName' => 'Ada']);

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
// The option keys the mapping gives Length are options it takes.
$length = $validator->getMetadataFor(User::class)->getPropertyMetadata('displayName')[0]->getConstraints()[0];
check('Length takes min and max from the mapping', $length instanceof Length && $length->min === 2 && $length->max === 50);

// ─── Doctrine mapping ───────────────────────────────────────────────────────

$mapping = simplexml_load_file("$config/doctrine/User.orm.xml");
$entity = (string) $mapping->entity['name'];
$repository = (string) $mapping->entity['repository-class'];
check('The XML mapping names the User entity', $entity === User::class);
check('The XML mapping names a ServiceEntityRepository', is_subclass_of($repository, ServiceEntityRepository::class));
check('The mapped repository declares withEmail()', method_exists($repository, 'withEmail'));
$metadata = new ClassMetadata(User::class);
(new SimplifiedXmlDriver(["$config/doctrine" => 'App\Entity'], '.orm.xml'))->loadMetadataForClass(User::class, $metadata);
check('The XML mapping maps email and displayName, which findOneByEmail() and findBy() take', $metadata->hasField('email') && $metadata->hasField('displayName'));
$entityManager = new EntityManager(
    DriverManager::getConnection(['driver' => 'pdo_sqlite', 'memory' => true]),
    ORMSetup::createXMLMetadataConfiguration(["$config/doctrine"], true),
);
$entityManager->getConfiguration()->setMetadataDriverImpl(new SimplifiedXmlDriver(["$config/doctrine" => 'App\Entity'], '.orm.xml'));
$dqlParses = static function (string $dql) use ($entityManager): bool {
    try {
        $entityManager->createQuery($dql)->getSQL();
        return true;
    } catch (QueryException) {
        return false;
    }
};
check('The DQL in UserRepository::named() names the User entity and its mapped fields', $dqlParses('SELECT u FROM App\Entity\User u WHERE u.displayName = :name ORDER BY u.email ASC'));
check('A field the mapping does not map fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u WHERE u.nickname = :name'));
check('An entity class without its identification variable fails DQL', !$dqlParses('SELECT u FROM App\Entity\User WHERE u.displayName = :name'));
check('A stray parenthesis in the where() fragment fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u WHERE u.displayName = :name)'));
$userMetadata = $entityManager->getClassMetadata(User::class);
check('The XML mapping maps team as an association to Team', $userMetadata->hasAssociation('team') && $userMetadata->getAssociationTargetClass('team') === Team::class);
check('The DQL in UserRepository::inTeam() joins Team and names its fields', $dqlParses('SELECT u FROM App\Entity\User u JOIN u.team t WHERE t.name = :team'));
check('A field the joined Team does not map fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u JOIN u.team t WHERE t.email = :team'));
check('A variable nothing declares fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u JOIN u.team t WHERE x.name = :team'));
check('UserRepository::search() without a team builds DQL without t', $dqlParses('SELECT u FROM App\Entity\User u'));
check('A variable used on a path that does not join it fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u WHERE t.name = :team'));
check('UserRepository::inTeamByEmail() builds DQL that joins t through joinTeam()', $dqlParses('SELECT u FROM App\Entity\User u INNER JOIN u.team t WHERE t.name = :team ORDER BY u.email ASC'));
check('A variable neither byEmail() nor joinTeam() declares fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u WHERE t.displayName = :name ORDER BY u.email ASC'));
check('UserRepository::teamMembers() builds DQL that joins t through byEmailInTeam()', $dqlParses('SELECT u FROM App\Entity\User u INNER JOIN u.team t WHERE t.name = :team ORDER BY u.email ASC'));
check('A variable no method along the chain declares fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u INNER JOIN u.team t WHERE x.name = :team ORDER BY u.email ASC'));
check('UserRepository::namedTeamMembers() builds DQL that joins the t onlyNamedTeams() uses', $dqlParses('SELECT u FROM App\Entity\User u INNER JOIN u.team t WHERE t.name IS NOT NULL ORDER BY u.email ASC'));
check('A variable a method the builder is passed to uses, and nothing declares, fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u WHERE t.name IS NOT NULL ORDER BY u.email ASC'));
(new SchemaTool($entityManager))->createSchema([
    $entityManager->getClassMetadata(User::class),
    $entityManager->getClassMetadata(Team::class),
]);
$core = new Team('Core');
$member = new User('ada@example.com', 'Ada');
$member->setTeam($core);
$entityManager->persist($core);
$entityManager->persist($member);
$entityManager->flush();
$inTeam = $entityManager->createQueryBuilder()
    ->select('u')
    ->from(User::class, 'u')
    ->join('u.team', 't')
    ->where('t.name = :team')
    ->setParameter('team', 'Core')
    ->getQuery()
    ->getResult();
check('inTeam() gets a list of User', array_is_list($inTeam) && $inTeam[0] instanceof User);
$rows = $entityManager->createQueryBuilder()
    ->select('u')
    ->from(User::class, 'u', 'u.email')
    ->select('u.email', 'u.displayName')
    ->getQuery()
    ->getResult();
check('displayNamesByEmail() gets a shape of the fields keyed by email', $rows === ['ada@example.com' => ['email' => 'ada@example.com', 'displayName' => 'Ada']]);
$rows = $entityManager
    ->createQuery('SELECT t.name, COUNT(u.email) AS members, UPPER(t.name) AS label, AVG(LENGTH(u.displayName)) AS nameLength FROM App\Entity\User u JOIN u.team t GROUP BY t.name')
    ->getResult();
check('teamSizes() gets the COUNT() as an int, UPPER() as a string, and AVG() on SQLite as a float', $rows === [['name' => 'Core', 'members' => 1, 'label' => 'CORE', 'nameLength' => 3.0]]);
$dbal = Yaml::parseFile("$config/packages/doctrine.yaml")['doctrine']['dbal'];
check('config/packages/doctrine.yaml connects with SQLite, which the assertions run on', str_starts_with($dbal['url'], 'sqlite:'));
$users = $entityManager->createQuery('SELECT u FROM App\Entity\User u WHERE u.displayName = :name')
    ->setParameter('name', 'Ada')
    ->getResult();
check('named() gets a list of User', array_is_list($users) && $users[0] instanceof User);
$builder = $entityManager->createQueryBuilder()->select('u')->from(User::class, 'u')->orderBy('u.email', 'ASC');
$builder->andWhere('u.displayName = :name')->setParameter('name', 'Ada');
$users = $builder->getQuery()->getResult();
check('searchByName() gets a list of User from the builder byEmail() sets up and the branch changes', array_is_list($users) && $users[0] instanceof User);
$users = $entityManager->createQueryBuilder()
    ->select('u')
    ->from(User::class, 'u')
    ->add('orderBy', new \Doctrine\ORM\Query\Expr\OrderBy('u.displayName', 'ASC'))
    ->getQuery()
    ->getResult();
check('sortedByName() orders by the displayName an Expr\OrderBy names', array_is_list($users) && $users[0] instanceof User);
check('A stray parenthesis in an Expr\OrderBy fails DQL', !$dqlParses('SELECT u FROM App\Entity\User u ORDER BY u.displayName) ASC'));

// ─── Summary ────────────────────────────────────────────────────────────────

echo "\n";
if ($failed === 0) {
    echo "\033[32m✓ All $passed assertions passed.\033[0m\n";
} else {
    echo "\033[31m✗ $failed failed, $passed passed.\033[0m\n";
    exit(1);
}
