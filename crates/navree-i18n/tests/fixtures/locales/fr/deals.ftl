deal-created = { $count ->
    [one] Une affaire créée
   *[other] { $count } affaires créées
}

deal-won = { $owner } a gagné { $title }.
deal-won-subject = Affaire gagnée : { $title }
deal-stage-negotiation = Négociation
deal-amount = { $amount }
deal-closes = Clôture le { $date }

pipeline-empty = Votre pipeline est vide. Créez une affaire pour commencer.
