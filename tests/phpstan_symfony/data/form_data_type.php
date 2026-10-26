<?php declare(strict_types = 1);

namespace GenericFormDataType;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\Form\AbstractType;
use Symfony\Component\Form\Extension\Core\Type\NumberType;
use Symfony\Component\Form\Extension\Core\Type\TextType;
use Symfony\Component\Form\FormBuilderInterface;
use Symfony\Component\Form\FormFactoryInterface;
use Symfony\Component\OptionsResolver\OptionsResolver;
use function PHPStan\Testing\assertType;

class DataClass
{

	/** @var int */
	public $foo;

	/** @var string */
	public $bar;

}

/**
 * @extends AbstractType<DataClass>
 */
class DataClassType extends AbstractType
{

	public function buildForm(FormBuilderInterface $builder, array $options): void
	{
		assertType('GenericFormDataType\DataClass|null', $builder->getData()); // SKIP: Symfony return types an argument decides
		assertType('GenericFormDataType\DataClass|null', $builder->getForm()->getData()); // SKIP: Symfony return types an argument decides

		$builder
			->add('foo', NumberType::class)
			->add('bar', TextType::class)
		;
	}

	public function configureOptions(OptionsResolver $resolver): void
	{
		$resolver
			->setDefaults([
				'data_class' => DataClass::class,
			])
		;
	}

}

class FormFactoryAwareClass
{

	/** @var FormFactoryInterface */
	private $formFactory;

	public function __construct(FormFactoryInterface $formFactory)
	{
		$this->formFactory = $formFactory;
	}

	public function doSomething(): void
	{
		$form = $this->formFactory->create(DataClassType::class, new DataClass());
		assertType('GenericFormDataType\DataClass', $form->getData()); // SKIP: Symfony return types an argument decides
	}

	public function doSomethingNullable(): void
	{
		$form = $this->formFactory->create(DataClassType::class);
		assertType('GenericFormDataType\DataClass|null', $form->getData()); // SKIP: Symfony return types an argument decides
	}

}

class FormController extends AbstractController
{

	public function doSomething(): void
	{
		$form = $this->createForm(DataClassType::class, new DataClass());
		assertType('GenericFormDataType\DataClass', $form->getData()); // SKIP: Symfony return types an argument decides
	}

	public function doSomethingNullable(): void
	{
		$form = $this->createForm(DataClassType::class);
		assertType('GenericFormDataType\DataClass|null', $form->getData()); // SKIP: Symfony return types an argument decides
	}

}
