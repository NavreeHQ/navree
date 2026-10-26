class Api::DealsController < ApplicationController
  include Authentication

  # GET /api/deals?page=P&per_page=S
  def index
    pagination = Pagination.new(params.permit(:page, :per_page))
    if pagination.invalid?
      return render json: { errors: pagination.errors }, status: :unprocessable_content
    end

    total = Deal.count
    deals = Deal.includes(:owner)
      .order(created_at: :desc, id: :desc)
      .limit(pagination.limit)
      .offset(pagination.offset)

    render json: { data: deals, meta: pagination.meta(total) }
  end

  # POST /api/deals
  def create
    deal = Current.user.deals.new(deal_params)

    if deal.save
      render json: deal, status: :created
    else
      render json: { errors: deal.errors }, status: :unprocessable_content
    end
  end

  private
    def deal_params
      params.expect(deal: [ :title, :amount_cents, :stage ])
    end
end
