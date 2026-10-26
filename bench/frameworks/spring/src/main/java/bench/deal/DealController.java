package bench.deal;

import bench.user.User;
import jakarta.validation.Valid;
import jakarta.validation.constraints.Max;
import jakarta.validation.constraints.Min;
import org.springframework.http.HttpStatus;
import org.springframework.security.core.annotation.AuthenticationPrincipal;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.ResponseStatus;
import org.springframework.web.bind.annotation.RestController;

/**
 * Authenticated deal endpoints. The constraints on the query parameters are checked by Spring MVC's
 * built-in method validation (Spring Framework 6.1+), which is why the class has no {@code @Validated}.
 */
@RestController
@RequestMapping("/api/deals")
public class DealController {

    private final DealService deals;

    public DealController(DealService deals) {
        this.deals = deals;
    }

    @GetMapping
    public DealPage list(
            @RequestParam(defaultValue = "1") @Min(1) int page,
            @RequestParam(name = "per_page", defaultValue = "20") @Min(1) @Max(100) int perPage) {
        return deals.list(page, perPage);
    }

    @PostMapping
    @ResponseStatus(HttpStatus.CREATED)
    public DealResponse create(@Valid @RequestBody CreateDealRequest request, @AuthenticationPrincipal User user) {
        return deals.create(request, user);
    }
}
