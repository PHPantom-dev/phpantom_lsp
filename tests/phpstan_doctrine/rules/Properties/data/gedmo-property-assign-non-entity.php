<?php // lint >= 8.1

namespace MissingGedmoWrittenPropertyAssign;

use Gedmo\Mapping\Annotation as Gedmo;

class NonEntityWithAGemdoLocaleField
{
    #[Gedmo\Locale]
    private string $locale; // ok, locale is written and read by gedmo listeners
}
