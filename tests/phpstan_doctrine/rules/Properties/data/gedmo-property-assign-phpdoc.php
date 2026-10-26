<?php // lint >= 7.4

namespace MissingGedmoWrittenPropertyAssignPhpDoc;

use Doctrine\ORM\Mapping as ORM;
use Gedmo\Mapping\Annotation as Gedmo;

#[ORM\Entity]
class EntityWithAPhpDocGemdoLocaleField
{
    /**
     * @Gedmo\Locale
     */
    private string $locale; // ok, locale is written and read by gedmo listeners
}
