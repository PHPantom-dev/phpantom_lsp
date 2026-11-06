<?php

declare(strict_types=1);

namespace Drupal\acme\Controller;

use Drupal\acme\Greeter;
use Drupal\Core\Controller\ControllerBase;
use Drupal\Core\Link;
use Drupal\Core\Url;
use Symfony\Component\HttpFoundation\RedirectResponse;

/**
 * Shows the greeting for the acme.greeting route.
 */
final class GreetingController extends ControllerBase
{
    // ControllerBase autowires this argument through the Greeter alias in
    // acme.services.yml.
    public function __construct(
        private readonly Greeter $greeter,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function greet(string $name): array
    {
        // Try: go to the definition of `$this->greeter->greet()`, or complete
        // after `$this->` to see the helpers ControllerBase inherits from core.
        return [
            '#theme' => 'acme_greeting',
            '#name' => $name,
            '#greeting' => $this->greeter->greet($name),
        ];
    }

    /**
     * Sends the visitor to the greeting for the world.
     */
    public function greetTheWorld(): RedirectResponse
    {
        // Try: go to the definition of 'acme.greeting', which lands on its key
        // in acme.routing.yml, or complete a route name inside redirect('').
        return $this->redirect('acme.greeting', ['name' => 'World']);
    }

    /**
     * A link to the greeting for the world.
     */
    public function worldLink(): Link
    {
        // Try: find the references of 'acme.greeting' to see every place that
        // links or redirects to it, the links and Twig files included. Go to
        // the definition of 'name', which lands on {name} in the route's path,
        // or complete a parameter key inside the array.
        $url = Url::fromRoute('acme.greeting', ['name' => 'World']);

        return Link::createFromRoute($this->t('Greet the world'), 'acme.greeting', $url->getRouteParameters());
    }

    /**
     * The greeting for `$name` in a card, with more about it folded away.
     *
     * @return array<string, mixed>
     */
    public function card(string $name): array
    {
        // Try: go to the definition of 'acme_greeting_card', which lands on
        // the GreetingCard class, or hover 'details' to see core's element
        // class. Complete an element name inside '#type' => '', core's
        // elements included.
        $build = [
            '#type' => 'acme_greeting_card',
            'greeting' => $this->greet($name),
        ];
        $build['more']['#type'] = 'details';
        $build['more']['#title'] = $this->t('More');

        return $build;
    }
}
