<?php // lint >= 8.1

namespace MissingGedmoWrittenPropertyAssign;

use Doctrine\ORM\Mapping as ORM;
use Gedmo\Mapping\Annotation as Gedmo;

#[ORM\Entity]
class EntityWithAGemdoLocaleField
{
    #[Gedmo\Locale]
    private string $locale; // ok, locale is written and read by gedmo listeners
}
