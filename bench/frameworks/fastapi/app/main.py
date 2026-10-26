"""FastAPI + SQLAlchemy 2 async ORM (asyncpg) + PyJWT: the benchmark API (bench/SPEC.md)."""

import math
import random
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from datetime import UTC, datetime
from typing import Annotated

from fastapi import Depends, FastAPI, Query, status
from sqlalchemy import func, select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import joinedload

from app.auth import CurrentUser, get_current_user
from app.database import SessionDep, engine
from app.models import Deal, World
from app.schemas import DealCreate, DealOut, DealPage, Message, WorldOut


@asynccontextmanager
async def lifespan(app: FastAPI) -> AsyncIterator[None]:
    yield
    await engine.dispose()


app = FastAPI(title="Keel benchmark (FastAPI)", lifespan=lifespan)


def random_world_id() -> int:
    return random.randint(1, 10000)


async def load_world(session: AsyncSession, world_id: int) -> World:
    # select() rather than session.get(): always one query, even for an id
    # already in this session's identity map.
    return (await session.execute(select(World).where(World.id == world_id))).scalar_one()


@app.get("/json", response_model=Message)
async def json_test():
    return {"message": "Hello, World!"}


@app.get("/db", response_model=WorldOut)
async def single_query(session: SessionDep):
    return await load_world(session, random_world_id())


@app.get("/queries", response_model=list[WorldOut])
async def multiple_queries(session: SessionDep, queries: str = "1"):
    # A missing or non-numeric value means 1; clamp to 1..500.
    try:
        count = min(500, max(1, int(queries)))
    except ValueError:
        count = 1
    return [await load_world(session, random_world_id()) for _ in range(count)]


@app.get("/api/deals", response_model=DealPage, dependencies=[Depends(get_current_user)])
async def list_deals(
    session: SessionDep,
    page: Annotated[int, Query(ge=1)] = 1,
    per_page: Annotated[int, Query(ge=1, le=100)] = 20,
):
    total = await session.scalar(select(func.count()).select_from(Deal))
    deals = await session.scalars(
        select(Deal)
        .options(joinedload(Deal.owner, innerjoin=True))
        .order_by(Deal.created_at.desc(), Deal.id.desc())
        .limit(per_page)
        .offset((page - 1) * per_page)
    )
    return {
        "data": deals.all(),
        "meta": {
            "current_page": page,
            "per_page": per_page,
            "total": total,
            "last_page": max(1, math.ceil(total / per_page)),
        },
    }


@app.post("/api/deals", response_model=DealOut, status_code=status.HTTP_201_CREATED)
async def create_deal(deal_in: DealCreate, user: CurrentUser, session: SessionDep):
    now = datetime.now(UTC)
    deal = Deal(**deal_in.model_dump(), owner=user, created_at=now, updated_at=now)
    session.add(deal)
    await session.commit()
    return deal
