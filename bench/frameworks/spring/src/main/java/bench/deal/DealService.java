package bench.deal;

import bench.user.User;
import bench.user.UserRepository;
import org.springframework.data.domain.Page;
import org.springframework.data.domain.PageRequest;
import org.springframework.data.domain.Sort;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

@Service
public class DealService {

    private static final Sort NEWEST_FIRST = Sort.by(Sort.Order.desc("createdAt"), Sort.Order.desc("id"));

    private final DealRepository deals;
    private final UserRepository users;

    public DealService(DealRepository deals, UserRepository users) {
        this.deals = deals;
        this.users = users;
    }

    /** Page {@code page} (1-based) of all deals, newest first. */
    @Transactional(readOnly = true)
    public DealPage list(int page, int perPage) {
        Page<Deal> result = deals.findAll(PageRequest.of(page - 1, perPage, NEWEST_FIRST));
        return DealPage.of(result, page, perPage);
    }

    @Transactional
    public DealResponse create(CreateDealRequest request, User owner) {
        // The owner was loaded during authentication; a reference avoids re-reading it.
        Deal deal = new Deal(request.title(), request.amountCents(), request.stage(), users.getReferenceById(owner.getId()));
        // Flush now (it would happen at commit anyway) so the INSERT sets the generated timestamps.
        deals.saveAndFlush(deal);
        return DealResponse.of(deal, owner);
    }
}
