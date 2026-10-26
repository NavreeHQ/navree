package bench.world;

import java.util.ArrayList;
import java.util.List;
import java.util.NoSuchElementException;
import java.util.concurrent.ThreadLocalRandom;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

@Service
@Transactional(readOnly = true)
public class WorldService {

    private static final int ROWS = 10_000;

    private final WorldRepository worlds;

    public WorldService(WorldRepository worlds) {
        this.worlds = worlds;
    }

    public WorldResponse randomWorld() {
        return WorldResponse.of(find(randomId()));
    }

    /** {@code count} separate single-row lookups, one after another, in one read-only transaction. */
    public List<WorldResponse> randomWorlds(int count) {
        List<WorldResponse> result = new ArrayList<>(count);
        for (int i = 0; i < count; i++) {
            result.add(WorldResponse.of(find(randomId())));
        }
        return result;
    }

    private World find(int id) {
        return worlds.findById(id).orElseThrow(() -> new NoSuchElementException("world " + id));
    }

    private static int randomId() {
        return ThreadLocalRandom.current().nextInt(1, ROWS + 1);
    }
}
