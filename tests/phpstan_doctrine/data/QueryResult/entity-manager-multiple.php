<?php declare(strict_types = 1);

// The two entity managers the multiple-manager fixtures run against, as
// the source that builds them, with the attribute mapping ORM 3 reads in
// place of upstream's annotations.

use Doctrine\DBAL\DriverManager;
use Doctrine\ORM\Configuration;
use Doctrine\ORM\EntityManager;
use Doctrine\ORM\Mapping\Driver\AttributeDriver;
use Doctrine\Persistence\ManagerRegistry;

$createEntityManager = static function (string $path): EntityManager {
	$config = new Configuration();
	$config->setProxyDir(__DIR__);
	$config->setProxyNamespace('PHPstan\Doctrine\OrmProxies');
	$config->setMetadataDriverImpl(new AttributeDriver([$path]));

	return new EntityManager(
		DriverManager::getConnection([
			'driver' => 'pdo_sqlite',
			'memory' => true,
		]),
		$config,
	);
};

$defaultManager = $createEntityManager(
	__DIR__ . '/EntitiesMultipleManagers/Main',
);
$tenantManager = $createEntityManager(
	__DIR__ . '/EntitiesMultipleManagers/Tenant',
);

return new class ($defaultManager, $tenantManager) implements ManagerRegistry {

	private EntityManager $defaultManager;

	private EntityManager $tenantManager;

	public function __construct(EntityManager $defaultManager, EntityManager $tenantManager)
	{
		$this->defaultManager = $defaultManager;
		$this->tenantManager = $tenantManager;
	}

	public function getDefaultConnectionName()
	{
		return 'default';
	}

	public function getConnection($name = null)
	{
		return $this->getManager($name)->getConnection();
	}

	public function getConnections()
	{
		return [
			'default' => $this->defaultManager->getConnection(),
			'tenant' => $this->tenantManager->getConnection(),
		];
	}

	public function getConnectionNames()
	{
		return [
			'default' => 'default',
			'tenant' => 'tenant',
		];
	}

	public function getDefaultManagerName()
	{
		return 'default';
	}

	public function getManager($name = null)
	{
		if ($name === 'tenant') {
			return $this->tenantManager;
		}

		return $this->defaultManager;
	}

	public function getManagers()
	{
		return [
			'default' => $this->defaultManager,
			'tenant' => $this->tenantManager,
		];
	}

	public function resetManager($name = null)
	{
		return $this->getManager($name);
	}

	public function getManagerNames()
	{
		return [
			'default' => 'default',
			'tenant' => 'tenant',
		];
	}

	public function getAliasNamespace($alias)
	{
		throw new LogicException('Alias namespaces are not used in this test fixture.');
	}

	public function getRepository($persistentObject, $persistentManagerName = null)
	{
		return $this->getManager($persistentManagerName)->getRepository($persistentObject);
	}

	public function getManagerForClass($class)
	{
		foreach ($this->getManagers() as $manager) {
			if (!$manager->getMetadataFactory()->isTransient($class)) {
				return $manager;
			}
		}

		return null;
	}

};
