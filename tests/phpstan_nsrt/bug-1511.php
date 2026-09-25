<?php

namespace Bug1511;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

class Response
{
	private $message;

	/** @throws void */
	public function __construct($message)
	{
		$this->message = $message;
	}

}

class Serializer
{

	public function serialize(): void
	{

	}

}

class Controller
{
	public function prepareResponse($date): Response
	{
		$serializer = new Serializer();
		$response = null;
		try {
			$serializer->serialize();
			// do something with response object e.g. serialize some entity
			$response = new Response("Success");
		} catch (\Throwable $e) {
			$response = new Response("Error");
		} finally {
			assertVariableCertainty(TrinaryLogic::createYes(), $response);
			assertType(Response::class, $response);
			return $response;
		}
	}
}
