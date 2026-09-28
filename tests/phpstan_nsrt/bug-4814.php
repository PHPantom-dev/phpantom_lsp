<?php

namespace Bug4814TypeInference;

use function PHPStan\Testing\assertType;

class Request {
	public function __construct(string $endpoint) { echo $endpoint; }
}

class Response {
	/** @return mixed */
	public function getBody() { return '{"json"}'; }
}

class Foo
{

	/** @throws \Throwable */
	public function sendRequest(Request $req) : Response {
		return new Response();
	}

	public function doFoo()
	{
		$body                = null;
		$decodedResponseBody = [];
		try {
			$request = new Request('endpoint');

			$response            = $this->sendRequest($request);
			$body                = (string) $response->getBody();
			$decodedResponseBody = json_decode($body, true, 512, JSON_THROW_ON_ERROR);
		} catch (\Throwable $exception) {
			assertType('string|null', $body);
			assertType('array{}', $decodedResponseBody);
		}
	}

}
