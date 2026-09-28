<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Casts\AsArrayObject;
use Illuminate\Database\Eloquent\Casts\AsBinary;
use Illuminate\Database\Eloquent\Casts\AsCollection;
use Illuminate\Database\Eloquent\Casts\AsEncryptedArrayObject;
use Illuminate\Database\Eloquent\Casts\AsEncryptedCollection;
use Illuminate\Database\Eloquent\Casts\AsEnumArrayObject;
use Illuminate\Database\Eloquent\Casts\AsEnumCollection;
use Illuminate\Database\Eloquent\Casts\AsFluent;
use Illuminate\Database\Eloquent\Casts\AsHtmlString;
use Illuminate\Database\Eloquent\Casts\AsStringable;
use Illuminate\Database\Eloquent\Casts\AsUri;
use Illuminate\Database\Eloquent\Model;

class CastSample extends Model
{
    protected function casts(): array
    {
        return [
            'blurb' => AsHtmlString::class,
            'directory' => AsStringable::class,
            'embedding' => AsBinary::uuid(),
            'flavors' => AsEnumArrayObject::of(JamFlavor::class),
            'homepage' => AsUri::class,
            'notes' => AsCollection::using(PostCollection::class),
            'options' => AsArrayObject::class,
            'secret_options' => AsEncryptedArrayObject::class,
            'secrets' => AsEncryptedCollection::of(Frosting::class),
            'settings' => AsFluent::class,
            'statuses' => AsEnumCollection::of(OrderStatus::class),
            'tags' => AsCollection::class,
            'toppings' => AsCollection::of(Frosting::class),
            'ulid' => AsBinary::ulid(),
        ];
    }
}
