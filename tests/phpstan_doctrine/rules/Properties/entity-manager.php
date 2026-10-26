<?php declare(strict_types = 1);

// The entity manager these rule fixtures run against, as the source that
// builds it, with the attribute mapping ORM 3 reads in place of upstream's
// annotations.

use Doctrine\DBAL\DriverManager;
use Doctrine\ORM\Configuration;
use Doctrine\ORM\EntityManager;
use Doctrine\ORM\Mapping\Driver\AttributeDriver;

$config = new Configuration();
$config->setProxyDir(__DIR__);
$config->setProxyNamespace('PHPstan\Doctrine\OrmProxies');
$config->setMetadataDriverImpl(new AttributeDriver([__DIR__ . '/data']));

return new EntityManager(
	DriverManager::getConnection([
		'driver' => 'pdo_sqlite',
		'memory' => true,
	]),
	$config,
);
