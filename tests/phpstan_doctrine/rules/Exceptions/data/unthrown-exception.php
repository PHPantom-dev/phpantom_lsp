<?php

namespace UnthrownException;

class FooFacade
{

	/** @var \Doctrine\ORM\EntityManager */
	private $entityManager;

	/** @var \Doctrine\ORM\EntityManagerInterface */
	private $entityManagerInterface;

	public function doFoo(): void
	{
		try {
			$this->entityManager->flush();
		} catch (\Doctrine\DBAL\Exception\UniqueConstraintViolationException $e) {
			// pass
		}
		try {
			$this->entityManagerInterface->flush();
		} catch (\Doctrine\DBAL\Exception\UniqueConstraintViolationException $e) {
			// pass
		}
	}

}
