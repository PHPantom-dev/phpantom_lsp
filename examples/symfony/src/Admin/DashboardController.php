<?php

namespace App\Admin;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\Routing\Attribute\Route;

final class DashboardController extends AbstractController
{
    // Try: find references on 'dashboard'. config/routes.yaml imports this
    // directory with the name_prefix admin_, so the route is
    // 'admin_dashboard', the name templates/demo/index.html.twig links to.
    #[Route('/admin', name: 'dashboard')]
    public function index(): Response
    {
        // Try: complete a route name inside redirectToRoute(''). The names
        // the imports put together are offered, and a name nothing declares,
        // such as 'dashboard' on its own, is reported.
        return $this->redirectToRoute('app_demo', ['userId' => 1]);
    }
}
