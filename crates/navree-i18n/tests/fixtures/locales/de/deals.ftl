deal-created = { $count ->
    [one] Ein Geschäft angelegt
   *[other] { $count } Geschäfte angelegt
}

deal-won = { $owner } hat { $title } gewonnen.
deal-won-subject = Gewonnen: { $title }
