import { Injectable, UnauthorizedException } from '@nestjs/common';
import { PassportStrategy } from '@nestjs/passport';
import { isUUID } from 'class-validator';
import { ExtractJwt, Strategy } from 'passport-jwt';
import { User } from '../users/user.entity.js';
import { UsersService } from '../users/users.service.js';

interface AccessTokenClaims {
  sub?: unknown;
}

// Bearer token, HS256, signature + exp + iss checked by passport-jwt, then the
// user row is loaded. Anything wrong ends in a 401 from AuthGuard('jwt').
@Injectable()
export class JwtStrategy extends PassportStrategy(Strategy) {
  constructor(private readonly users: UsersService) {
    super({
      jwtFromRequest: ExtractJwt.fromAuthHeaderAsBearerToken(),
      secretOrKey: process.env.JWT_SECRET as string,
      issuer: process.env.JWT_ISSUER ?? 'keel',
      algorithms: ['HS256'],
      ignoreExpiration: false,
    });
  }

  async validate(claims: AccessTokenClaims): Promise<User> {
    // 'loose': any canonical UUID, as PostgreSQL's uuid type accepts (the
    // seeded ids are md5 hashes, not RFC 9562 versions).
    if (typeof claims.sub !== 'string' || !isUUID(claims.sub, 'loose')) {
      throw new UnauthorizedException();
    }
    const user = await this.users.findById(claims.sub);
    if (!user) throw new UnauthorizedException();
    return user;
  }
}
