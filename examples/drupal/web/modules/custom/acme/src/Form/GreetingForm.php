<?php

declare(strict_types=1);

namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;
use Drupal\Core\Form\FormStateInterface;

/**
 * Asks for a name to greet, with a callback of each shape Drupal reads.
 */
final class GreetingForm extends FormBase
{
    public function getFormId(): string
    {
        return 'acme_greeting_form';
    }

    /**
     * @param array<string, mixed> $form
     * @return array<string, mixed>
     */
    public function buildForm(array $form, FormStateInterface $form_state): array
    {
        // Try: go to the definition of validateName after the `::`, which
        // lands on the method of this form: Drupal calls a '::method'
        // callback on the form object. Find the references of
        // validateName(), or rename it, and the string follows. Delete
        // validateName and complete after the `::`: the form's public
        // methods are offered.
        $form['#validate'][] = '::validateName';
        $form['name'] = [
            '#type' => 'textfield',
            '#title' => 'Name',
            // Try: hover trimName. A class name joined to '::method' reads
            // as one 'Class::method'.
            '#value_callback' => static::class . '::trimName',
            // Try: go to the definition of refreshGreeting, the callback
            // the `#ajax` array holds.
            '#ajax' => ['callback' => '::refreshGreeting'],
        ];
        // Try: go to the definition of addHint. The class name in a
        // 'Class::method' string is fully qualified, whatever the file
        // imports.
        $form['#after_build'] = ['Drupal\acme\Form\GreetingForm::addHint'];
        $form['actions']['submit'] = [
            '#type' => 'submit',
            '#value' => 'Greet',
            // Try: go to the definition of acme_greeting_form_submit, a
            // function of acme.module. A function name in a callback string
            // is global, whatever namespace the file is in.
            '#submit' => ['::submitForm', 'acme_greeting_form_submit'],
        ];

        return $form;
    }

    /**
     * @param array<string, mixed> $form
     */
    public function validateName(array &$form, FormStateInterface $form_state): void
    {
        if (trim((string) $form_state->getValue('name')) === '') {
            $form_state->setErrorByName('name', 'Enter a name.');
        }
    }

    /**
     * @param array<string, mixed> $element
     */
    public static function trimName(array &$element, mixed $input, FormStateInterface $form_state): mixed
    {
        return is_string($input) ? trim($input) : null;
    }

    /**
     * @param array<string, mixed> $form
     * @return array<string, mixed>
     */
    public function refreshGreeting(array &$form, FormStateInterface $form_state): array
    {
        return $form['name'];
    }

    /**
     * @param array<string, mixed> $form
     * @return array<string, mixed>
     */
    public static function addHint(array $form, FormStateInterface $form_state): array
    {
        $form['name']['#description'] = 'The name the greeting uses.';

        return $form;
    }

    /**
     * @param array<string, mixed> $form
     */
    public function submitForm(array &$form, FormStateInterface $form_state): void
    {
        $form_state->setRedirect('acme.greeting', ['name' => $form_state->getValue('name')]);
    }
}
