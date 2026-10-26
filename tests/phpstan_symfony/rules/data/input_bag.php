<?php

namespace InputBagTest;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\Routing\Annotation\Route;

class TestController extends AbstractController
{
	/**
	 * @Route("/test", name="test")
	 */
	public function index(Request $request): Response
	{
		$foo = $request->query->get('foo');

		return $this->render('test/index.html.twig', [
			'controller_name' => 'TestController',
		]);
	}
}
