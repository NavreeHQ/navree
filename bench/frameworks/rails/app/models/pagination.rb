# Form object for the `page` / `per_page` query parameters of a paginated list.
class Pagination
  include ActiveModel::API

  MAX_PER_PAGE = 100

  # Kept as the raw query-string values, so numericality rejects "abc" or "1.5"
  # instead of seeing them type-cast to 0 or 1.
  attr_accessor :page, :per_page

  validates :page, numericality: { only_integer: true, greater_than_or_equal_to: 1 }
  validates :per_page, numericality: { only_integer: true, in: 1..MAX_PER_PAGE }

  def initialize(attributes = {})
    self.page = 1
    self.per_page = 20
    super
  end

  def limit
    per_page.to_i
  end

  def offset
    (page.to_i - 1) * limit
  end

  def meta(total)
    { current_page: page.to_i, per_page: limit, total: total, last_page: [ total.fdiv(limit).ceil, 1 ].max }
  end
end
