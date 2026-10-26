<?php declare(strict_types=1);

use Symfony\Component\Form\FormError;
use Symfony\Component\Form\FormErrorIterator;
use Symfony\Component\Form\FormInterface;
use function PHPStan\Testing\assertType;

/** @var FormInterface $form */
$form = new stdClass();

assertType(FormErrorIterator::class . '<'. FormError::class .'>', $form->getErrors()); // SKIP: Symfony return types an argument decides
assertType(FormErrorIterator::class . '<'. FormError::class .'>', $form->getErrors(false)); // SKIP: Symfony return types an argument decides
assertType(FormErrorIterator::class . '<'. FormError::class .'>', $form->getErrors(false, true)); // SKIP: Symfony return types an argument decides

assertType(FormErrorIterator::class . '<'. FormError::class .'>', $form->getErrors(true)); // SKIP: Symfony return types an argument decides
assertType(FormErrorIterator::class . '<'. FormError::class .'>', $form->getErrors(true, true)); // SKIP: Symfony return types an argument decides

assertType(FormErrorIterator::class . '<'. FormError::class .'>', $form->getErrors(false, false)); // SKIP: Symfony return types an argument decides

assertType(FormErrorIterator::class . '<'. FormError::class .'|'. FormErrorIterator::class . '>', $form->getErrors(true, false)); // SKIP: Symfony return types an argument decides
