import { Exclude, Expose } from 'class-transformer';
import { Column, Entity, JoinColumn, ManyToOne, PrimaryColumn, type ValueTransformer } from 'typeorm';
import { User } from '../users/user.entity.js';

export const DEAL_STAGES = ['lead', 'qualified', 'proposal', 'negotiation', 'won', 'lost'] as const;
export type DealStage = (typeof DEAL_STAGES)[number];

// node-postgres returns bigint as a string; amounts stay far below 2^53.
const bigintToNumber: ValueTransformer = {
  to: (value: number) => value,
  from: (value: string | null) => (value === null ? null : Number(value)),
};

// Serialized (ClassSerializerInterceptor) in the API's snake_case shape.
@Exclude()
@Entity('deals')
export class Deal {
  @Expose()
  @PrimaryColumn('uuid')
  id: string;

  @Expose()
  @Column('varchar', { length: 200 })
  title: string;

  @Expose({ name: 'amount_cents' })
  @Column('bigint', { name: 'amount_cents', transformer: bigintToNumber })
  amountCents: number;

  @Expose()
  @Column('varchar', { length: 20 })
  stage: DealStage;

  @Expose()
  @ManyToOne(() => User, { nullable: false })
  @JoinColumn({ name: 'owner_id' })
  owner: User;

  @Expose({ name: 'created_at' })
  @Column('timestamptz', { name: 'created_at' })
  createdAt: Date;

  @Column('timestamptz', { name: 'updated_at' })
  updatedAt: Date;
}
