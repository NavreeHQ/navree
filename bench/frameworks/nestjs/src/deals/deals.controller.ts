import { Body, ClassSerializerInterceptor, Controller, Get, Post, Query, UseGuards, UseInterceptors } from '@nestjs/common';
import { AuthGuard } from '@nestjs/passport';
import { CurrentUser } from '../auth/current-user.decorator.js';
import { User } from '../users/user.entity.js';
import { Deal } from './deal.entity.js';
import { DealsService, type Paginated } from './deals.service.js';
import { CreateDealDto } from './dto/create-deal.dto.js';
import { ListDealsQuery } from './dto/list-deals.query.js';

@Controller('api/deals')
@UseGuards(AuthGuard('jwt'))
@UseInterceptors(ClassSerializerInterceptor)
export class DealsController {
  constructor(private readonly deals: DealsService) {}

  @Get()
  list(@Query() query: ListDealsQuery): Promise<Paginated<Deal>> {
    return this.deals.paginate(query.page, query.per_page);
  }

  @Post()
  create(@Body() dto: CreateDealDto, @CurrentUser() user: User): Promise<Deal> {
    return this.deals.create(dto, user);
  }
}
