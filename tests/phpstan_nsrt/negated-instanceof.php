<?php

namespace NegatedInstanceof;

use function PHPStan\Testing\assertType;

class Ty
{
    public function start(): void {}
}
class CallableTy extends Ty {}
class ClosureTy extends Ty {}

class Idn {}
class VarLikeIdn extends Idn {}
class Expr {}

class Nm {}

class FuncCall
{
    /** @var Nm|Expr */
    public $name;
}

class Holder
{
    /** @var Ty */
    public $t;
}

/**
 * De Morgan: the guard rules out everything the disjunction rules in, so
 * the fall-through is the disjunction itself — as alternatives, not as an
 * intersection.
 */
function guardOnNegatedDisjunction(Ty $type): void
{
    if (!($type instanceof CallableTy || $type instanceof ClosureTy)) {
        return;
    }
    assertType('NegatedInstanceof\\CallableTy|NegatedInstanceof\\ClosureTy', $type);
}

function branchesOfNegatedDisjunction(Ty $type): void
{
    if (!($type instanceof CallableTy || $type instanceof ClosureTy)) {
        assertType('NegatedInstanceof\\Ty', $type);
    } else {
        assertType('NegatedInstanceof\\CallableTy|NegatedInstanceof\\ClosureTy', $type);
    }
}

/** A later check refines what the guard left, rather than starting over. */
function guardThenNegatedCheck(Ty $type): void
{
    if (!($type instanceof CallableTy || $type instanceof ClosureTy)) {
        return;
    }
    $type->start();
    if (!$type instanceof CallableTy) {
        assertType('NegatedInstanceof\\ClosureTy', $type);
    } else {
        assertType('NegatedInstanceof\\CallableTy', $type);
    }
}

function negatedDisjunctionOnProperty(Holder $h): void
{
    if (!($h->t instanceof CallableTy || $h->t instanceof ClosureTy)) {
        return;
    }
    assertType('NegatedInstanceof\\CallableTy|NegatedInstanceof\\ClosureTy', $h->t);
}

/**
 * A failed check rules out the class it names and every subclass of it,
 * and a passing one keeps a subclass as the more specific of the two.
 *
 * @param Idn|VarLikeIdn|Expr $name
 */
function subclassSubtraction($name): void
{
    if ($name instanceof Idn) {
        assertType('NegatedInstanceof\\Idn|NegatedInstanceof\\VarLikeIdn', $name);
    } else {
        assertType('NegatedInstanceof\\Expr', $name);
    }
}

/** An exact identity check pins the class, so the subclass does not pass. */
function exactIdentityDropsSubclass(Idn $name): void
{
    if (get_class($name) === Idn::class) {
        assertType('NegatedInstanceof\\Idn', $name);
    }
}

function negatedInstanceofOnPropertyInAndChain(?FuncCall $expr): void
{
    if ($expr instanceof FuncCall && !$expr->name instanceof Nm) {
        assertType('NegatedInstanceof\\Expr', $expr->name);
    }
}
