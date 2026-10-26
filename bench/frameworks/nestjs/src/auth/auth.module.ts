import { Module } from '@nestjs/common';
import { PassportModule } from '@nestjs/passport';
import { UsersModule } from '../users/users.module.js';
import { JwtStrategy } from './jwt.strategy.js';

@Module({
  imports: [PassportModule, UsersModule],
  providers: [JwtStrategy],
})
export class AuthModule {}
