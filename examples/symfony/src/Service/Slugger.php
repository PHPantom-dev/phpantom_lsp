<?php

namespace App\Service;

class Slugger
{
    public function slug(string $text): string
    {
        return strtolower(trim((string) preg_replace('/[^A-Za-z0-9]+/', '-', $text), '-'));
    }
}
