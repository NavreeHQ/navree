class ApplicationController < ActionController::API
  # Missing params and malformed JSON bodies: a JSON 400 rather than the default empty error page.
  rescue_from ActionController::ParameterMissing, ActionDispatch::Http::Parameters::ParseError do
    render json: { error: "Bad Request" }, status: :bad_request
  end
end
