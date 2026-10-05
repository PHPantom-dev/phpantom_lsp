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
        #[Autowire(param: 'app.sender_name')]
        private readonly string $senderName,
    ) {}

    #[Route('/demo/{userId}', name: 'app_demo')]
    public function index(ContainerInterface $container, string $userId): Response
    {
        // Try completion, navigation, references, and rename in these strings.
        $container->has('app.welcome_mailer');
        $this->generateUrl('app_demo', ['userId' => $userId]);
        $title = $this->translator->trans('demo.title');
        $this->events->dispatch(new OrderPlaced($userId), 'app.order.placed');
        $this->commandBus->dispatch(new SendWelcomeEmail($userId));

        return $this->render('demo/index.html.twig', [
            'sender' => $this->senderName,
            'title' => $title,
        ]);
    }

    public function legacy(): Response
    {
        return new Response('Configured in routes.yaml');
    }
}
