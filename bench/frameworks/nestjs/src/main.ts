// NestJS on Express or Fastify (HTTP_ADAPTER), one Nest app per CPU (SPEC.md §4).
import cluster from 'node:cluster';
import { availableParallelism } from 'node:os';
import { type INestApplication, type NestApplicationOptions, ValidationPipe } from '@nestjs/common';
import { NestFactory } from '@nestjs/core';
import { AppModule } from './app.module.js';

const options: NestApplicationOptions = { logger: ['error', 'warn'] };

async function createApp(): Promise<INestApplication> {
  if (process.env.HTTP_ADAPTER === 'fastify') {
    const { FastifyAdapter } = await import('@nestjs/platform-fastify');
    return NestFactory.create(AppModule, new FastifyAdapter(), options);
  }
  const { ExpressAdapter } = await import('@nestjs/platform-express');
  return NestFactory.create(AppModule, new ExpressAdapter(), options);
}

async function bootstrap(): Promise<void> {
  const app = await createApp();
  app.useGlobalPipes(new ValidationPipe({ transform: true }));
  await app.listen(8080, '0.0.0.0');
}

// Nest has no clustering of its own: the primary forks one worker per CPU,
// and the workers share port 8080 through node:cluster.
if (cluster.isPrimary) {
  for (let i = 0; i < availableParallelism(); i++) cluster.fork();
  cluster.on('exit', () => cluster.fork());
} else {
  await bootstrap();
}
