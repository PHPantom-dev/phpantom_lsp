<?php

namespace App\Form;

use App\Entity\User;
use Symfony\Component\Form\AbstractType;
use Symfony\Component\Form\Extension\Core\Type\EmailType;
use Symfony\Component\Form\FormBuilderInterface;
use Symfony\Component\OptionsResolver\OptionsResolver;
use Symfony\Component\Validator\Constraints as Assert;

final class RegistrationType extends AbstractType
{
    /**
     * @param array<string, mixed> $options
     */
    public function buildForm(FormBuilderInterface $builder, array $options): void
    {
        $builder
            // Try: complete an option inside a new '' key of the options
            // array, or misspell 'help' to see it reported. EmailType takes
            // the options of the types it extends and of the type
            // extensions too, such as the validator's 'constraints'.
            ->add('email', EmailType::class, [
                'help' => 'We never share it.',
                // Try: misspell max. A constraint's options are its
                // constructor's parameters.
                'constraints' => [new Assert\Length(max: 180)],
            ])
            ->add('displayName');
    }

    public function configureOptions(OptionsResolver $resolver): void
    {
        $resolver->setDefaults(['data_class' => User::class]);
    }
}
