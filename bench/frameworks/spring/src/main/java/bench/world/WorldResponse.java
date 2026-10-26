package bench.world;

public record WorldResponse(int id, int randomNumber) {

    static WorldResponse of(World world) {
        return new WorldResponse(world.getId(), world.getRandomNumber());
    }
}
