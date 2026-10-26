package bench.deal;

import java.util.List;
import org.springframework.data.domain.Page;
import tools.jackson.databind.PropertyNamingStrategies;
import tools.jackson.databind.annotation.JsonNaming;

public record DealPage(List<DealResponse> data, Meta meta) {

    @JsonNaming(PropertyNamingStrategies.SnakeCaseStrategy.class)
    public record Meta(int currentPage, int perPage, long total, long lastPage) {
    }

    static DealPage of(Page<Deal> page, int currentPage, int perPage) {
        List<DealResponse> data = page.getContent().stream().map(DealResponse::of).toList();
        long total = page.getTotalElements();
        long lastPage = Math.max(1, (total + perPage - 1) / perPage);
        return new DealPage(data, new Meta(currentPage, perPage, total, lastPage));
    }
}
