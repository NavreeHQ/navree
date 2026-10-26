package bench.deal;

import jakarta.validation.constraints.Max;
import jakarta.validation.constraints.Min;
import jakarta.validation.constraints.NotNull;
import jakarta.validation.constraints.Pattern;
import jakarta.validation.constraints.Size;
import tools.jackson.databind.PropertyNamingStrategies;
import tools.jackson.databind.annotation.JsonNaming;

@JsonNaming(PropertyNamingStrategies.SnakeCaseStrategy.class)
public record CreateDealRequest(
        @NotNull @Size(min = 1, max = 200)
        String title,

        @NotNull @Min(0) @Max(1_000_000_000_000L)
        Long amountCents,

        @NotNull @Pattern(regexp = "lead|qualified|proposal|negotiation|won|lost", message = "must be one of lead, qualified, proposal, negotiation, won, lost")
        String stage) {
}
