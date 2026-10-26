<?php declare(strict_types = 1);

// The entity manager the QueryResult fixtures run against, as the source
// that builds it. PHPStan's Doctrine extension boots it to read the
// mapping; PHPantom reads the same mapping from the entities' attributes.
// Upstream maps the entities with annotations, which ORM 3 no longer reads.

use Doctrine\DBAL\DriverManager;
use Doctrine\ORM\Configuration;
use Doctrine\ORM\EntityManager;
use Doctrine\ORM\Mapping\Driver\AttributeDriver;
use Doctrine\Persistence\Mapping\Driver\MappingDriverChain;

$config = new Configuration();
$config->setProxyDir(__DIR__);
$config->setProxyNamespace('PHPstan\Doctrine\OrmProxies');

$metadataDriver = new MappingDriverChain();
$metadataDriver->addDriver(new AttributeDriver([__DIR__ . '/Entities']), 'QueryResult\Entities\\');
$metadataDriver->addDriver(new AttributeDriver([__DIR__ . '/EntitiesEnum']), 'QueryResult\EntitiesEnum\\');
$metadataDriver->addDriver(new AttributeDriver([__DIR__ . '/EntitiesDbal42']), 'QueryResult\EntitiesDbal42\\');

$config->setMetadataDriverImpl($metadataDriver);

return new EntityManager(
	DriverManager::getConnection([
		'driver' => 'pdo_sqlite',
		'memory' => true,
	]),
	$config
);
