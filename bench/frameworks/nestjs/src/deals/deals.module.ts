import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { DealsController } from './deals.controller.js';
import { Deal } from './deal.entity.js';
import { DealsService } from './deals.service.js';

@Module({
  imports: [TypeOrmModule.forFeature([Deal])],
  controllers: [DealsController],
  providers: [DealsService],
})
export class DealsModule {}
