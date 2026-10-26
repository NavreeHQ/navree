<?php

use App\Http\Controllers\BenchmarkController;
use App\Http\Controllers\DealController;
use Illuminate\Support\Facades\Route;

Route::get('/json', [BenchmarkController::class, 'json']);
Route::get('/db', [BenchmarkController::class, 'db']);
Route::get('/queries', [BenchmarkController::class, 'queries']);

Route::prefix('api')->middleware('auth:api')->group(function () {
    Route::apiResource('deals', DealController::class)->only(['index', 'store']);
});
