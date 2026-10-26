<?php declare(strict_types = 1);

use Doctrine\ORM\Mapping\ClassMetadata;
use Doctrine\Persistence\ManagerRegistry;
use Doctrine\Persistence\ObjectManager;
use Doctrine\Persistence\Mapping\ClassMetadataFactory;
use NamedManagerRepositoryInference\DefaultSharedRepository;
use NamedManagerRepositoryInference\SharedEntity;
use NamedManagerRepositoryInference\TenantSharedRepository;

require_once __DIR__ . '/namedManagerRepository.php';

$createManager = static function (string $repositoryClass): ObjectManager {
	$metadata = new ClassMetadata(SharedEntity::class);
	$metadata->customRepositoryClassName = $repositoryClass;

	$metadataFactory = new class ($metadata) implements ClassMetadataFactory {

		private ClassMetadata $metadata;

		public function __construct(ClassMetadata $metadata)
		{
			$this->metadata = $metadata;
		}

		public function getAllMetadata()
		{
			return [$this->metadata];
		}

		public function getMetadataFor($className)
		{
			return $this->metadata;
		}

		public function hasMetadataFor($className)
		{
			return $className === SharedEntity::class;
		}

		public function setMetadataFor($className, $class)
		{
		}

		public function isTransient($className)
		{
			return $className !== SharedEntity::class;
		}

	};

	return new class ($metadata, $metadataFactory) implements ObjectManager {

		private ClassMetadata $metadata;

		private ClassMetadataFactory $metadataFactory;

		public function __construct(ClassMetadata $metadata, ClassMetadataFactory $metadataFactory)
		{
			$this->metadata = $metadata;
			$this->metadataFactory = $metadataFactory;
		}

		public function find($className, $id)
		{
			return null;
		}

		public function persist($object)
		{
		}

		public function merge($object)
		{
			return $object;
		}

		public function remove($object)
		{
		}

		public function clear($objectName = null)
		{
		}

		public function detach($object)
		{
		}

		public function refresh($object)
		{
		}

		public function flush()
		{
		}

		public function getRepository($className)
		{
			throw new LogicException('Repository instances are not needed by this type inference fixture.');
		}

		public function getClassMetadata($className)
		{
			return $this->metadata;
		}

		public function getMetadataFactory()
		{
			return $this->metadataFactory;
		}

		public function initializeObject($obj)
		{
		}

		public function contains($object)
		{
			return false;
		}

	};
};

$defaultManager = $createManager(DefaultSharedRepository::class);
$tenantManager = $createManager(TenantSharedRepository::class);

return new class ($defaultManager, $tenantManager) implements ManagerRegistry {

	private ObjectManager $defaultManager;

	private ObjectManager $tenantManager;

	public function __construct(ObjectManager $defaultManager, ObjectManager $tenantManager)
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
		throw new LogicException('Connections are not used in this type inference fixture.');
	}

	public function getConnections()
	{
		return [];
	}

	public function getConnectionNames()
	{
		return [];
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

	public function getRepository($persistentObject, $persistentManagerName = null)
	{
		return $this->getManager($persistentManagerName)->getRepository($persistentObject);
	}

	public function getManagerForClass($class)
	{
		return $class === SharedEntity::class ? $this->defaultManager : null;
	}

	public function getAliasNamespace($alias)
	{
		throw new LogicException('Alias namespaces are not used in this type inference fixture.');
	}

};
