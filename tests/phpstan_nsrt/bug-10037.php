<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug10037;

interface Identifier
{}

interface Document
{}

/** @template T of Identifier */
interface Fetcher
{
	/** @phpstan-assert-if-true T $identifier */
	public function supports(Identifier $identifier): bool;

	/** @param T $identifier */
	public function fetch(Identifier $identifier): Document;
}

/** @implements Fetcher<PostIdentifier> */
final readonly class PostFetcher implements Fetcher
{
	public function supports(Identifier $identifier): bool
	{
		return $identifier instanceof PostIdentifier;
	}

	public function fetch(Identifier $identifier): Document
	{
		// SA knows $identifier is instance of PostIdentifier here
		return $identifier->foo();
	}
}

class PostIdentifier implements Identifier
{
	public function foo(): Document
	{
		return new class implements Document{};
	}
}

function (Identifier $i): void {
	$fetcher = new PostFetcher();
	\PHPStan\Testing\assertType('Bug10037\Identifier', $i);
	if ($fetcher->supports($i)) {
		\PHPStan\Testing\assertType('Bug10037\PostIdentifier', $i);
		$fetcher->fetch($i);
	} else {
		$fetcher->fetch($i);
	}
};

class Post
{
}

/** @template T */
abstract class Voter
{

	/** @phpstan-assert-if-true T $subject */
	abstract function supports(string $attribute, mixed $subject): bool;

	/** @param T $subject */
	abstract function voteOnAttribute(string $attribute, mixed $subject): bool;

}

/** @extends Voter<Post> */
class PostVoter extends Voter
{

	/** @phpstan-assert-if-true Post $subject */
	function supports(string $attribute, mixed $subject): bool
	{

	}

	function voteOnAttribute(string $attribute, mixed $subject): bool
	{
		\PHPStan\Testing\assertType('Bug10037\Post', $subject);
	}
}

function ($subject): void {
	$voter = new PostVoter();
	\PHPStan\Testing\assertType('mixed', $subject);
	if ($voter->supports('aaa', $subject)) {
		\PHPStan\Testing\assertType('Bug10037\Post', $subject);
		$voter->voteOnAttribute('aaa', $subject);
	} else {
		$voter->voteOnAttribute('aaa', $subject);
	}
};
