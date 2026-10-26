"""Models over the existing benchmark tables (bench/db/init.sql); Django never migrates them."""

import uuid

from django.conf import settings
from django.contrib.auth.base_user import AbstractBaseUser, BaseUserManager
from django.core.validators import MaxValueValidator, MinValueValidator
from django.db import models


class World(models.Model):
    id = models.IntegerField(primary_key=True)
    randomnumber = models.IntegerField()

    class Meta:
        managed = False
        db_table = "world"


class UserManager(BaseUserManager):
    def create_user(self, email, name, password=None, **extra_fields):
        user = self.model(email=self.normalize_email(email), name=name, **extra_fields)
        user.set_password(password)
        user.save(using=self._db)
        return user


class User(AbstractBaseUser):
    """Navree's `users` table as the AUTH_USER_MODEL.

    The table names the hash column `password_hash` and has no `last_login` or
    `is_active` columns: `password` is mapped onto `password_hash`,
    `last_login` is removed, and `is_active` stays AbstractBaseUser's constant True.
    """

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    name = models.CharField(max_length=255)
    email = models.EmailField(max_length=255, unique=True)
    password = models.CharField(max_length=255, db_column="password_hash")
    last_login = None
    email_verified_at = models.DateTimeField(null=True, blank=True)
    two_factor_secret = models.TextField(null=True, blank=True)
    two_factor_recovery_codes = models.TextField(null=True, blank=True)
    two_factor_confirmed_at = models.DateTimeField(null=True, blank=True)
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    USERNAME_FIELD = "email"
    EMAIL_FIELD = "email"
    REQUIRED_FIELDS = ["name"]

    objects = UserManager()

    class Meta:
        managed = False
        db_table = "users"


class Deal(models.Model):
    class Stage(models.TextChoices):
        LEAD = "lead"
        QUALIFIED = "qualified"
        PROPOSAL = "proposal"
        NEGOTIATION = "negotiation"
        WON = "won"
        LOST = "lost"

    id = models.UUIDField(primary_key=True, default=uuid.uuid4, editable=False)
    title = models.CharField(max_length=200)
    amount_cents = models.BigIntegerField(
        validators=[MinValueValidator(0), MaxValueValidator(1_000_000_000_000)]
    )
    stage = models.CharField(max_length=20, choices=Stage)
    owner = models.ForeignKey(settings.AUTH_USER_MODEL, on_delete=models.CASCADE, related_name="deals")
    created_at = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)

    class Meta:
        managed = False
        db_table = "deals"
