"""Pydantic v2 request and response models."""

import uuid
from datetime import datetime
from typing import Annotated, Literal

from pydantic import BaseModel, Field

Stage = Literal["lead", "qualified", "proposal", "negotiation", "won", "lost"]


class Message(BaseModel):
    message: str


class WorldOut(BaseModel):
    id: int
    randomNumber: int = Field(validation_alias="randomnumber")


class OwnerOut(BaseModel):
    id: uuid.UUID
    name: str


class DealOut(BaseModel):
    id: uuid.UUID
    title: str
    amount_cents: int
    stage: Stage
    owner: OwnerOut
    created_at: datetime


class PageMeta(BaseModel):
    current_page: int
    per_page: int
    total: int
    last_page: int


class DealPage(BaseModel):
    data: list[DealOut]
    meta: PageMeta


class DealCreate(BaseModel):
    title: Annotated[str, Field(min_length=1, max_length=200)]
    amount_cents: Annotated[int, Field(ge=0, le=1_000_000_000_000)]
    stage: Stage
