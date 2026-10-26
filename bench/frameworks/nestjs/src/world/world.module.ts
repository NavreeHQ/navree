import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { WorldController } from './world.controller.js';
import { World } from './world.entity.js';
import { WorldService } from './world.service.js';

@Module({
  imports: [TypeOrmModule.forFeature([World])],
  controllers: [WorldController],
  providers: [WorldService],
})
export class WorldModule {}
