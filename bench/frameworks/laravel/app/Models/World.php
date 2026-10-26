<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Attributes\Table;
use Illuminate\Database\Eloquent\Model;

/**
 * @property int $id
 * @property int $randomnumber
 */
#[Table('world', timestamps: false)]
class World extends Model
{
}
