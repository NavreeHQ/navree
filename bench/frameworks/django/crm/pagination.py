import math

from rest_framework.pagination import BasePagination
from rest_framework.response import Response

from crm.serializers import PageParamsSerializer


class PagePagination(BasePagination):
    """`?page=&per_page=` pagination rendered as `{"data": [...], "meta": {...}}`.

    Unlike PageNumberPagination, invalid parameters are a 400 (validated by a
    serializer) rather than a 404 or a silent clamp, and a page past the end is
    an empty 200.
    """

    def paginate_queryset(self, queryset, request, view=None):
        params = PageParamsSerializer(data=request.query_params)
        params.is_valid(raise_exception=True)
        self.page = params.validated_data["page"]
        self.per_page = params.validated_data["per_page"]
        self.total = queryset.count()
        offset = (self.page - 1) * self.per_page
        return list(queryset[offset : offset + self.per_page])

    def get_paginated_response(self, data):
        return Response(
            {
                "data": data,
                "meta": {
                    "current_page": self.page,
                    "per_page": self.per_page,
                    "total": self.total,
                    "last_page": max(1, math.ceil(self.total / self.per_page)),
                },
            }
        )
