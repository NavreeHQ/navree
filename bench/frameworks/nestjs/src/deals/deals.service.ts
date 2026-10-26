import { randomUUID } from 'node:crypto';
import { Injectable } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import type { User } from '../users/user.entity.js';
import { Deal } from './deal.entity.js';
import type { CreateDealDto } from './dto/create-deal.dto.js';

export interface Paginated<T> {
  data: T[];
  meta: { current_page: number; per_page: number; total: number; last_page: number };
}

@Injectable()
export class DealsService {
  constructor(@InjectRepository(Deal) private readonly deals: Repository<Deal>) {}

  /** Newest first, owners joined in (no query per deal), plus the total count. */
  async paginate(page: number, perPage: number): Promise<Paginated<Deal>> {
    const [data, total] = await this.deals.findAndCount({
      relations: { owner: true },
      order: { createdAt: 'DESC', id: 'DESC' },
      skip: (page - 1) * perPage,
      take: perPage,
    });
    return {
      data,
      meta: { current_page: page, per_page: perPage, total, last_page: Math.max(1, Math.ceil(total / perPage)) },
    };
  }

  async create(dto: CreateDealDto, owner: User): Promise<Deal> {
    // The schema has no column defaults, so the id and timestamps are set
    // here; insert() writes the new row without save()'s existence check.
    const now = new Date();
    const deal = this.deals.create({
      id: randomUUID(),
      title: dto.title,
      amountCents: dto.amount_cents,
      stage: dto.stage,
      owner,
      createdAt: now,
      updatedAt: now,
    });
    await this.deals.insert(deal);
    return deal;
  }
}
