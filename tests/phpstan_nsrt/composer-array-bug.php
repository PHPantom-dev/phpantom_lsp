<?php

namespace ComposerArrayBug;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var mixed[] */
	private $config;

	/** @var string[] */
	private $errors;

	public function doFoo(): void
	{
		if (!empty($this->config['authors'])) {
			foreach ($this->config['authors'] as $key => $author) {
				assertType("mixed", $this->config['authors']);

				if (!is_array($author)) {
					$this->errors[] = 'authors.'.$key.' : should be an array, '.gettype($author).' given';
					assertType("mixed", $this->config['authors']);
					unset($this->config['authors'][$key]);
					assertType("mixed", $this->config['authors']);
					continue;
				}
				assertType("mixed", $this->config['authors']);
				foreach (['homepage', 'email', 'name', 'role'] as $authorData) {
					if (isset($author[$authorData]) && !is_string($author[$authorData])) {
						$this->errors[] = 'authors.'.$key.'.'.$authorData.' : invalid value, must be a string';
						unset($this->config['authors'][$key][$authorData]);
					}
				}
				if (isset($author['homepage'])) {
					assertType("mixed", $this->config['authors']);
					unset($this->config['authors'][$key]['homepage']);
					assertType("mixed", $this->config['authors']);
				}
				if (isset($author['email']) && !filter_var($author['email'], FILTER_VALIDATE_EMAIL)) {
					unset($this->config['authors'][$key]['email']);
				}
				if (empty($this->config['authors'][$key])) {
					unset($this->config['authors'][$key]);
				}
			}

			assertType("mixed", $this->config['authors']);

			if (empty($this->config['authors'])) {
				unset($this->config['authors']);
			} else {
			}

			assertType("array<mixed>", $this->config);
		}
	}

}
