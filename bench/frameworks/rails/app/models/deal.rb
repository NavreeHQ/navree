class Deal < ApplicationRecord
  STAGES = %w[lead qualified proposal negotiation won lost].freeze

  belongs_to :owner, class_name: "User", inverse_of: :deals

  validates :title, presence: true, length: { maximum: 200 }
  validates :amount_cents, numericality: { only_integer: true, in: 0..1_000_000_000_000 }
  validates :stage, inclusion: { in: STAGES }

  # deals.id is a uuid primary key without a database default.
  before_create { self.id ||= SecureRandom.uuid }

  # The API representation: { id, title, amount_cents, stage, owner: { id, name }, created_at }.
  def serializable_hash(options = nil)
    super({ only: %i[id title amount_cents stage created_at], include: { owner: { only: %i[id name] } } }.merge(options || {}))
  end
end
