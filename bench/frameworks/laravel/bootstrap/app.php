<?php

use Illuminate\Foundation\Application;
use Illuminate\Foundation\Configuration\Exceptions;
use Illuminate\Foundation\Configuration\Middleware;
use Illuminate\Http\Request;

return Application::configure(basePath: dirname(__DIR__))
    ->withRouting(
        // Every route lives in routes/api.php (the stateless "api" middleware
        // group). The prefix is empty because the benchmark paths are /json,
        // /db and /queries next to /api/deals; the deals routes add "api".
        api: __DIR__.'/../routes/api.php',
        commands: __DIR__.'/../routes/console.php',
        apiPrefix: '',
    )
    ->withMiddleware(function (Middleware $middleware): void {
        // API only: there is no login page to send guests to, so the auth
        // middleware answers 401 instead of resolving route('login').
        $middleware->redirectGuestsTo(null);
    })
    ->withExceptions(function (Exceptions $exceptions): void {
        // Clients don't have to send "Accept: application/json": API errors
        // (401, 404, 422, ...) always render as JSON.
        $exceptions->shouldRenderJsonWhen(
            fn (Request $request) => $request->is('api/*') || $request->expectsJson(),
        );
    })->create();
