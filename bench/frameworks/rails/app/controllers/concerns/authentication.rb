# Bearer-token authentication: an HS256 JWT (signature, `exp` and `iss` checked)
# whose `sub` is a user id. Sets Current.user, or responds 401.
module Authentication
  extend ActiveSupport::Concern

  included do
    include ActionController::HttpAuthentication::Token::ControllerMethods

    before_action :require_authentication
  end

  private
    def require_authentication
      Current.user = authenticate_with_http_token { |token, _options| user_from_token(token) }
      render json: { error: "Unauthorized" }, status: :unauthorized unless Current.user
    end

    def user_from_token(token)
      jwt = Rails.configuration.x.jwt
      payload, _header = JWT.decode(token, jwt.secret, true, algorithm: "HS256", iss: jwt.issuer, verify_iss: true)
      User.find_by(id: payload["sub"])
    rescue JWT::DecodeError
      nil
    end
end
