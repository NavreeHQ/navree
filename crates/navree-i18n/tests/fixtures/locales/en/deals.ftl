deal-created = { $count ->
    [one] One deal created
   *[other] { $count } deals created
}

deal-won = { $owner } won { $title }.
deal-won-subject = You won { $title }
deal-stage-negotiation = Negotiation
deal-amount = { $amount }
deal-closes = Closes { $date }
deal-closes-long = Closes { DATETIME($date, dateStyle: "long") }
deal-total = Total: { NUMBER($total, style: "currency", currency: "EUR") }

pipeline-empty = Your pipeline is empty. Create a deal to get started.
