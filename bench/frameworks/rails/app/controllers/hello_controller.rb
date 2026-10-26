class HelloController < ApplicationController
  # GET /json
  def show
    render json: { message: "Hello, World!" }
  end
end
