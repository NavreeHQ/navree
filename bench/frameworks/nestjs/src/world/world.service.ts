import { Injectable } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { World } from './world.entity.js';

const WORLD_ROWS = 10_000;

@Injectable()
export class WorldService {
  constructor(@InjectRepository(World) private readonly worlds: Repository<World>) {}

  findRandom(): Promise<World> {
    return this.worlds.findOneByOrFail({ id: Math.floor(Math.random() * WORLD_ROWS) + 1 });
  }

  /** N separate single-row queries, run concurrently on the pool. */
  findManyRandom(count: number): Promise<World[]> {
    return Promise.all(Array.from({ length: count }, () => this.findRandom()));
  }
}
