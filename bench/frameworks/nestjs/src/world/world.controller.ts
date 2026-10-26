import { Controller, Get, Query } from '@nestjs/common';
import { World } from './world.entity.js';
import { WorldService } from './world.service.js';

@Controller()
export class WorldController {
  constructor(private readonly world: WorldService) {}

  @Get('db')
  db(): Promise<World> {
    return this.world.findRandom();
  }

  // A missing or non-numeric count means 1, so this is lenient parsing
  // rather than a validation error.
  @Get('queries')
  queries(@Query('queries') queries?: string): Promise<World[]> {
    const count = Math.min(500, Math.max(1, Number.parseInt(queries ?? '', 10) || 1));
    return this.world.findManyRandom(count);
  }
}
