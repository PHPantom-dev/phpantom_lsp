<?php

namespace App\Models;

class ReportingCustomer extends Customer
{
    protected $table = 'customers';

    // Try: complete or navigate mysql against config/database.php.
    protected $connection = 'mysql';
}
