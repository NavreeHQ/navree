"""ORM models for the existing tables (the schema is created by bench/db/init.sql)."""

import uuid
from datetime import datetime

from sqlalchemy import BigInteger, DateTime, ForeignKey, String, Text
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, relationship


class Base(DeclarativeBase):
    type_annotation_map = {datetime: DateTime(timezone=True)}


class World(Base):
    __tablename__ = "world"

    id: Mapped[int] = mapped_column(primary_key=True)
    randomnumber: Mapped[int]


class User(Base):
    __tablename__ = "users"

    id: Mapped[uuid.UUID] = mapped_column(primary_key=True)
    name: Mapped[str]
    email: Mapped[str] = mapped_column(unique=True)
    password_hash: Mapped[str]
    email_verified_at: Mapped[datetime | None]
    two_factor_secret: Mapped[str | None] = mapped_column(Text)
    two_factor_recovery_codes: Mapped[str | None] = mapped_column(Text)
    two_factor_confirmed_at: Mapped[datetime | None]
    created_at: Mapped[datetime]
    updated_at: Mapped[datetime]


class Deal(Base):
    __tablename__ = "deals"

    id: Mapped[uuid.UUID] = mapped_column(primary_key=True, default=uuid.uuid4)
    title: Mapped[str] = mapped_column(String(200))
    amount_cents: Mapped[int] = mapped_column(BigInteger)
    stage: Mapped[str] = mapped_column(String(20))
    owner_id: Mapped[uuid.UUID] = mapped_column(ForeignKey("users.id"))
    created_at: Mapped[datetime]
    updated_at: Mapped[datetime]

    # Never lazy-load: queries must eager-load the owner explicitly.
    owner: Mapped[User] = relationship(lazy="raise")
