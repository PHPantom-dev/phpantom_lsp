<?php

namespace App\Controller;

use App\Event\OrderPlaced;
use App\Message\SendWelcomeEmail;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\DependencyInjection\Attribute\Autowire;
use Symfony\Component\DependencyInjection\ContainerInterface;
use Symfony\Component\EventDispatcher\EventDispatcherInterface;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\Messenger\MessageBusInterface;
use Symfony\Component\Routing\Attribute\Route;
use Symfony\Contracts\Translation\TranslatorInterface;

final class DemoController extends AbstractController
{
    public function __construct(
        private readonly TranslatorInterface $translator,
        private readonly EventDispatcherInterface $events,
        private readonly MessageBusInterface $commandBus,
        // Try: go-to-definition on the parameter name, declared in
        // config/services.php.
        #[Autowire(param: 'app.sender_name')]
        private readonly string $senderName,
    ) {}

    // Try: find references on 'app_demo'. The URL generated below and the
    // link in templates/demo/index.html.twig both name it.
    #[Route('/demo/{userId}', name: 'app_demo')]
    public function index(ContainerInterface $container, string $userId): Response
    {
        // Try: complete a service ID inside has(''), or a parameter inside
        // getParameter(''). The receiver's type is what makes these names.
        $container->has('app.welcome_mailer');
        $supportEmail = $this->getParameter('app.support_email');
        // Try: hover $mailer and $supportEmail, or complete after $mailer->.
        // app.mailer is an alias of a service config/services.php builds
        // from WelcomeMailer, and the parameter's value in
        // config/services.yaml is a string.
        $mailer = $container->get('app.mailer');
        $mailer->send($userId);
        // Try: complete a route name inside generateUrl('').
        $this->generateUrl('app_demo', ['userId' => $userId]);
        // Try: go-to-definition on a translation key, or complete one inside
        // trans(''). A key is looked up in the domain the call names, so
        // 'user.invalid_email' is found in translations/validators.en.xlf.
        $title = $this->translator->trans('demo.title');
        $this->translator->trans('user.invalid_email', [], 'validators');
        // Try: go-to-definition on the event name. It lists every listener
        // that subscribes to it: the #[AsEventListener] in
        // src/EventListener/OrderPlacedListener.php and the tag in
        // config/services.yaml.
        $this->events->dispatch(new OrderPlaced($userId), 'app.order.placed');
        $this->commandBus->dispatch(new SendWelcomeEmail($userId));

        // Try: go-to-definition on the template name, or complete one inside
        // render('').
        return $this->render('demo/index.html.twig', [
            'sender' => $this->senderName,
            'support' => $supportEmail,
            'title' => $title,
        ]);
    }

    // Try: find references on legacy(). The route in config/routes.yaml points
    // here, and go-to-definition on it there lands back on this method.
    public function legacy(): Response
    {
        return new Response('Configured in routes.yaml');
    }
}
