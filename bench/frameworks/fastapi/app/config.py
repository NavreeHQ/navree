import os

from pydantic_settings import BaseSettings

# 64 connections in total (SPEC §4), split evenly over the Uvicorn workers.
# The Dockerfile sets WEB_CONCURRENCY to the CPU count (8 under the bench cpuset).
MAX_DB_CONNECTIONS = 64


class Settings(BaseSettings):
    db_host: str = "localhost"
    db_port: int = 5432
    db_name: str = "bench"
    db_user: str = "bench"
    db_password: str = "bench"
    jwt_secret: str
    jwt_issuer: str = "keel"
    web_concurrency: int = os.process_cpu_count() or 1

    @property
    def db_pool_size(self) -> int:
        return max(1, MAX_DB_CONNECTIONS // self.web_concurrency)


settings = Settings()
