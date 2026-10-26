import { Exclude, Expose } from 'class-transformer';
import { Column, CreateDateColumn, Entity, PrimaryColumn, UpdateDateColumn } from 'typeorm';

// Navis's users table. Serialized (ClassSerializerInterceptor) as { id, name } only.
@Exclude()
@Entity('users')
export class User {
  @Expose()
  @PrimaryColumn('uuid')
  id: string;

  @Expose()
  @Column('varchar')
  name: string;

  @Column('varchar', { unique: true })
  email: string;

  @Column('varchar', { name: 'password_hash' })
  passwordHash: string;

  @Column('timestamptz', { name: 'email_verified_at', nullable: true })
  emailVerifiedAt: Date | null;

  @Column('text', { name: 'two_factor_secret', nullable: true })
  twoFactorSecret: string | null;

  @Column('text', { name: 'two_factor_recovery_codes', nullable: true })
  twoFactorRecoveryCodes: string | null;

  @Column('timestamptz', { name: 'two_factor_confirmed_at', nullable: true })
  twoFactorConfirmedAt: Date | null;

  @CreateDateColumn({ name: 'created_at', type: 'timestamptz' })
  createdAt: Date;

  @UpdateDateColumn({ name: 'updated_at', type: 'timestamptz' })
  updatedAt: Date;
}
