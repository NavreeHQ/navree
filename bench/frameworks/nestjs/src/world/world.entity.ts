import { Column, Entity, PrimaryColumn } from 'typeorm';

@Entity('world')
export class World {
  @PrimaryColumn('integer')
  id: number;

  @Column('integer', { name: 'randomnumber' })
  randomNumber: number;
}
