<?php

namespace App\DependencyInjection;

use App\Service\WelcomeMailer;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\DependencyInjection\Extension\Extension;

final class AcmeDemoExtension extends Extension
{
    /**
     * @param array<array-key, mixed> $configs
     */
    public function load(array $configs, ContainerBuilder $container): void
    {
        /** @var array{api_key?: string, mailer?: array{dsn?: string}} $config */
        $config = $this->processConfiguration(new Configuration(), $configs);

        // Try: find references on 'app.dsn_mailer'. The alias below and the
        // lookup in DemoController name it. Services and parameters the
        // container builder declares navigate like the configuration's.
        $container->setParameter('app.mailer_dsn', $config['mailer']['dsn'] ?? 'null://null');
        $container->register('app.dsn_mailer', WelcomeMailer::class);
        $container->setAlias('app.default_mailer', 'app.dsn_mailer');
    }
}
