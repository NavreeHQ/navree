<?php

namespace App\Auth\Claims;

use PHPOpenSourceSaver\JWTAuth\Claims\Issuer as BaseIssuer;
use PHPOpenSourceSaver\JWTAuth\Exceptions\TokenInvalidException;

/**
 * The "iss" claim, checked against config('jwt.issuer').
 *
 * jwt-auth validates exp, iat and nbf but has no issuer option, so this
 * replaces its Issuer claim class (see AppServiceProvider). An invalid
 * token makes the jwt guard return no user, and the auth middleware 401s.
 */
class Issuer extends BaseIssuer
{
    public function validatePayload()
    {
        if ($this->getValue() !== config('jwt.issuer')) {
            throw new TokenInvalidException('Token issuer is not accepted');
        }

        return true;
    }
}
