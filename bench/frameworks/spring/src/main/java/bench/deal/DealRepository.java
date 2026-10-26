package bench.deal;

import java.util.UUID;
import org.springframework.data.domain.Page;
import org.springframework.data.domain.Pageable;
import org.springframework.data.jpa.repository.EntityGraph;
import org.springframework.data.jpa.repository.JpaRepository;

public interface DealRepository extends JpaRepository<Deal, UUID> {

    /** A page of deals with their owners fetched in the same query (a join), plus a count query. */
    @Override
    @EntityGraph(attributePaths = "owner")
    Page<Deal> findAll(Pageable pageable);
}
