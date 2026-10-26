<?php declare(strict_types = 1);

// The entity manager the fixture runs against, as the source that builds
// it: what the result's row count can be depends on the connection's
// driver.

use Doctrine\DBAL\DriverManager;
use Doctrine\ORM\Configuration;
use Doctrine\ORM\EntityManager;
use Doctrine\ORM\Mapping\Driver\AttributeDriver;

$config = new Configuration();
$config->setProxyDir(__DIR__);
$config->setProxyNamespace('App\GeneratedProxy');
$config->setMetadataDriverImpl(new AttributeDriver([__DIR__]));

return new EntityManager(
	DriverManager::getConnection([
		'driver' => 'pdo_pgsql',
		'memory' => true,
	]),
	$config,
);
