<?php

namespace PHPStan\Rules\Doctrine\ORMAttributes\Bug383;

use Doctrine\ORM\Mapping as ORM;
use Doctrine\Common\Collections\Collection;

#[ORM\Entity]
class Campus {

	#[ORM\Column(type: 'int')]
	#[ORM\Id]
	private int $id;

	/**
	 * @var Collection<int, Student>
	 */
	#[ORM\OneToMany(mappedBy: 'campus', targetEntity: Student::class)]
	private Collection $students;

	// .......
}

#[ORM\Entity]
class Student {
	// ......
	#[ORM\ManyToOne(targetEntity: Campus::class, inversedBy: 'students')]
	#[ORM\JoinColumn(nullable: false)]
	private Campus $campus;
	// .......
}
