class WorldsController < ApplicationController
  # GET /db
  def show
    render json: world_json(World.find(random_id))
  end

  # GET /queries?queries=N: N separate single-row queries, N clamped to 1..500.
  def index
    count = params[:queries].to_s.to_i.clamp(1, 500)
    # Bypass the per-request query cache so a repeated random id still reads the database.
    worlds = World.uncached { Array.new(count) { World.find(random_id) } }
    render json: worlds.map { |world| world_json(world) }
  end

  private
    def random_id
      rand(1..10_000)
    end

    def world_json(world)
      { id: world.id, randomNumber: world.randomnumber }
    end
end
