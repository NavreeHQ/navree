package bench.deal;

import bench.user.User;
import java.time.Instant;
import java.util.UUID;
import tools.jackson.databind.PropertyNamingStrategies;
import tools.jackson.databind.annotation.JsonNaming;

@JsonNaming(PropertyNamingStrategies.SnakeCaseStrategy.class)
public record DealResponse(UUID id, String title, long amountCents, String stage, Owner owner, Instant createdAt) {

    public record Owner(UUID id, String name) {
    }

    static DealResponse of(Deal deal) {
        return of(deal, deal.getOwner());
    }

    static DealResponse of(Deal deal, User owner) {
        return new DealResponse(
                deal.getId(),
                deal.getTitle(),
                deal.getAmountCents(),
                deal.getStage(),
                new Owner(owner.getId(), owner.getName()),
                deal.getCreatedAt());
    }
}
