<?php

namespace Symfony\Component\DependencyInjection\Loader\Configurator;

use App\Service\WelcomeMailer;

return static function (ContainerConfigurator $container): void {
    $services = $container->services();
    $parameters = $container->parameters();

    $services->set('app.welcome_mailer', WelcomeMailer::class);
    $services->alias('app.mailer', 'app.welcome_mailer');
    $parameters->set('app.sender_name', 'PHPantom');
};
