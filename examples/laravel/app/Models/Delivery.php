<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Concerns\HasUlids;
use Illuminate\Database\Eloquent\Model;

class Delivery extends Model
{
    use HasUlids;

    protected $primaryKey = 'tracking_id';
}
