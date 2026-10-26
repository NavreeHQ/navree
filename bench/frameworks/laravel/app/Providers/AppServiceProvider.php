<?php

namespace App\Providers;

use App\Auth\Claims\Issuer;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\ServiceProvider;
use PHPOpenSourceSaver\JWTAuth\Claims\Factory as ClaimFactory;

class AppServiceProvider extends ServiceProvider
{
    /**
     * Register any application services.
     */
    public function register(): void
    {
        // Validate the token's issuer (jwt-auth has no setting for it).
        $this->app->extend('tymon.jwt.claim.factory', fn (ClaimFactory $factory) => $factory->extend('iss', Issuer::class));
    }

    /**
     * Bootstrap any application services.
     */
    public function boot(): void
    {
        // A single resource is the bare object; paginated collections keep "data".
        JsonResource::withoutWrapping();
    }
}
