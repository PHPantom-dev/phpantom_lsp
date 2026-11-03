<?php

namespace Symfony\Component\DependencyInjection\Loader\Configurator;

use App\Service\WelcomeMailer;

return static function (ContainerConfigurator $container): void {
    $services = $container->services();
    $parameters = $container->parameters();

    // Try: find references on 'app.welcome_mailer'. The alias below, the
    // YAML in config/services.yaml, and the lookup in DemoController all
    // name it.
    $services->set('app.welcome_mailer', WelcomeMailer::class);
    $services->alias('app.mailer', 'app.welcome_mailer');
    $parameters->set('app.sender_name', 'PHPantom');
    // Try: go-to-definition on the placeholder. It names the parameter set
    // on the line above.
    $parameters->set('app.greeting', 'Hello from %app.sender_name%');

    // Try: go-to-definition on 'cache.app'. FrameworkBundle declares it in
    // its own Resources/config/cache.php. Complete inside parent('') to see
    // every service the installed bundles declare.
    $services->set('app.demo_cache')->parent('cache.app');
};
