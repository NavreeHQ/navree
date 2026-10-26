import { availableParallelism } from 'node:os';
import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AppController } from './app.controller.js';
import { AuthModule } from './auth/auth.module.js';
import { DealsModule } from './deals/deals.module.js';
import { WorldModule } from './world/world.module.js';

// At most 64 connections across all workers (SPEC.md §4).
const MAX_DB_CONNECTIONS = 64;
const POOL_SIZE = Math.max(1, Math.floor(MAX_DB_CONNECTIONS / availableParallelism()));

@Module({
  imports: [
    TypeOrmModule.forRoot({
      type: 'postgres',
      host: process.env.DB_HOST,
      port: Number(process.env.DB_PORT ?? 5432),
      username: process.env.DB_USER,
      password: process.env.DB_PASSWORD,
      database: process.env.DB_NAME,
      autoLoadEntities: true,
      // The schema already exists: never synchronize or migrate at boot.
      synchronize: false,
      migrationsRun: false,
      logging: false,
      extra: { max: POOL_SIZE },
    }),
    WorldModule,
    AuthModule,
    DealsModule,
  ],
  controllers: [AppController],
})
export class AppModule {}
