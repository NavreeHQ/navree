class User < ApplicationRecord
  has_many :deals, foreign_key: :owner_id, inverse_of: :owner
end
