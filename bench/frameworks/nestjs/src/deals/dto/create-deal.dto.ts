import { IsIn, IsInt, IsString, Length, Max, Min } from 'class-validator';
import { DEAL_STAGES, type DealStage } from '../deal.entity.js';

export class CreateDealDto {
  @IsString()
  @Length(1, 200)
  title: string;

  @IsInt()
  @Min(0)
  @Max(1_000_000_000_000)
  amount_cents: number;

  @IsIn(DEAL_STAGES)
  stage: DealStage;
}
