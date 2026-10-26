from rest_framework import serializers

from crm.models import Deal, User, World


class WorldSerializer(serializers.ModelSerializer):
    randomNumber = serializers.IntegerField(source="randomnumber")

    class Meta:
        model = World
        fields = ["id", "randomNumber"]


class OwnerSerializer(serializers.ModelSerializer):
    class Meta:
        model = User
        fields = ["id", "name"]


class DealSerializer(serializers.ModelSerializer):
    """Validates the create body (title, amount_cents, stage) and renders a deal."""

    owner = OwnerSerializer(read_only=True)

    class Meta:
        model = Deal
        fields = ["id", "title", "amount_cents", "stage", "owner", "created_at"]


class PageParamsSerializer(serializers.Serializer):
    page = serializers.IntegerField(min_value=1, default=1)
    per_page = serializers.IntegerField(min_value=1, max_value=100, default=20)
