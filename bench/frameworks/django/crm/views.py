import random

from rest_framework import generics
from rest_framework.permissions import AllowAny
from rest_framework.response import Response
from rest_framework.views import APIView

from crm.models import Deal, World
from crm.pagination import PagePagination
from crm.serializers import DealSerializer, WorldSerializer


def random_world_id():
    return random.randint(1, 10000)


class PublicAPIView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]


class JsonView(PublicAPIView):
    def get(self, request):
        return Response({"message": "Hello, World!"})


class SingleQueryView(PublicAPIView):
    def get(self, request):
        world = World.objects.get(id=random_world_id())
        return Response(WorldSerializer(world).data)


class MultipleQueriesView(PublicAPIView):
    def get(self, request):
        # A missing or non-numeric value means 1; clamp to 1..500.
        try:
            count = min(500, max(1, int(request.query_params.get("queries", 1))))
        except ValueError:
            count = 1
        worlds = [World.objects.get(id=random_world_id()) for _ in range(count)]
        return Response(WorldSerializer(worlds, many=True).data)


class DealListCreateView(generics.ListCreateAPIView):
    """GET: paginated deals, newest first, owners joined. POST: create a deal owned by the caller."""

    queryset = Deal.objects.select_related("owner").order_by("-created_at", "-id")
    serializer_class = DealSerializer
    pagination_class = PagePagination

    def perform_create(self, serializer):
        serializer.save(owner=self.request.user)
