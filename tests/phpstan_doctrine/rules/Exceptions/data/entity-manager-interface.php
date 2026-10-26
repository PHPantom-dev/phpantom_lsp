<?php

namespace EntityManagerInterfaceThrowTypeExtensionTest;

use Doctrine\DBAL\Exception\UniqueConstraintViolationException;
use Doctrine\ORM\EntityManagerInterface;
use Doctrine\ORM\Exception\ORMException;

class Example
{

	/**
	 * @throws ORMException
	 * @throws UniqueConstraintViolationException
	 */
	public function doFoo(EntityManagerInterface $entityManager): void
	{
		$entityManager->flush();
	}

}
