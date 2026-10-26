package bench.world;

import java.util.List;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class WorldController {

    private final WorldService worlds;

    public WorldController(WorldService worlds) {
        this.worlds = worlds;
    }

    public record Message(String message) {
    }

    @GetMapping("/json")
    public Message json() {
        return new Message("Hello, World!");
    }

    @GetMapping("/db")
    public WorldResponse db() {
        return worlds.randomWorld();
    }

    @GetMapping("/queries")
    public List<WorldResponse> queries(@RequestParam(required = false) String queries) {
        return worlds.randomWorlds(queryCount(queries));
    }

    /** 1..500; a missing or non-numeric value means 1. */
    static int queryCount(String value) {
        long count;
        try {
            count = Long.parseLong(value);
        } catch (NumberFormatException e) {
            return 1;
        }
        return (int) Math.min(500, Math.max(1, count));
    }
}
