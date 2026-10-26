"""Production settings for the Navree benchmark's Django + DRF app (bench/SPEC.md)."""

import os
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent

SECRET_KEY = os.environ.get("DJANGO_SECRET_KEY", "bench-django-secret-key-not-used-for-jwt-0123456789")
DEBUG = False
ALLOWED_HOSTS = ["*"]

INSTALLED_APPS = [
    "django.contrib.auth",
    "django.contrib.contenttypes",
    "rest_framework",
    "crm",
]

# startproject's middleware minus sessions, auth and messages: this is a
# stateless JSON API whose users authenticate with bearer JWTs in DRF.
MIDDLEWARE = [
    "django.middleware.security.SecurityMiddleware",
    "django.middleware.common.CommonMiddleware",
    "django.middleware.csrf.CsrfViewMiddleware",
    "django.middleware.clickjacking.XFrameOptionsMiddleware",
]

ROOT_URLCONF = "benchsite.urls"
WSGI_APPLICATION = "benchsite.wsgi.application"

# Persistent connections, one per Gunicorn thread: 17 workers x 3 threads = 51
# connections at most (SPEC §4 allows 64). See gunicorn.conf.py.
DATABASES = {
    "default": {
        "ENGINE": "django.db.backends.postgresql",
        "HOST": os.environ.get("DB_HOST", "localhost"),
        "PORT": os.environ.get("DB_PORT", "5432"),
        "NAME": os.environ.get("DB_NAME", "bench"),
        "USER": os.environ.get("DB_USER", "bench"),
        "PASSWORD": os.environ.get("DB_PASSWORD", "bench"),
        "CONN_MAX_AGE": None,
    }
}

AUTH_USER_MODEL = "crm.User"

LANGUAGE_CODE = "en-us"
TIME_ZONE = "UTC"
USE_I18N = True
USE_TZ = True

REST_FRAMEWORK = {
    "DEFAULT_AUTHENTICATION_CLASSES": ["rest_framework_simplejwt.authentication.JWTAuthentication"],
    "DEFAULT_PERMISSION_CLASSES": ["rest_framework.permissions.IsAuthenticated"],
    "DEFAULT_RENDERER_CLASSES": ["rest_framework.renderers.JSONRenderer"],
    "DEFAULT_PARSER_CLASSES": ["rest_framework.parsers.JSONParser"],
}

# Validates the benchmark's (Navree's) access tokens: HS256 with JWT_SECRET,
# iss == "navree", exp, a jti, typ == "access", and the user id in `sub`.
SIMPLE_JWT = {
    "ALGORITHM": "HS256",
    "SIGNING_KEY": os.environ["JWT_SECRET"],
    "ISSUER": os.environ.get("JWT_ISSUER", "navree"),
    "AUTH_HEADER_TYPES": ("Bearer",),
    "USER_ID_FIELD": "id",
    "USER_ID_CLAIM": "sub",
    "TOKEN_TYPE_CLAIM": "typ",
    "JTI_CLAIM": "jti",
    "UPDATE_LAST_LOGIN": False,
}
