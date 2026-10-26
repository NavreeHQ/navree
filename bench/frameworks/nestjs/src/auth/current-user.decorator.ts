import { createParamDecorator, type ExecutionContext } from '@nestjs/common';
import type { User } from '../users/user.entity.js';

/** The user that JwtStrategy.validate() attached to the request. */
export const CurrentUser = createParamDecorator(
  (_data: unknown, ctx: ExecutionContext): User => ctx.switchToHttp().getRequest<{ user: User }>().user,
);
