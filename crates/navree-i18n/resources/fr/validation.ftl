# Validation messages in French, shipped with navree-i18n.
#
# Each id is `validation-` and a rule key with dots as hyphens
# (`min.string` is `validation-min-string`). Variables: $attribute (the
# field's display name, from `field-<path>`), and the rule's own: $min,
# $max, $other, $values, $value, $format, $pattern.

validation-alpha_num = { $attribute } ne doit contenir que des lettres et des chiffres
validation-array = { $attribute } doit être un tableau
validation-ascii = { $attribute } ne doit contenir que des caractères ASCII
validation-boolean = { $attribute } doit être un booléen
validation-confirmed = la confirmation de { $attribute } ne correspond pas
validation-contains = { $attribute } doit contenir la chaîne { $value }
validation-country = { $attribute } doit être un code pays ISO 3166-1 alpha-2 valide
validation-credit_card = { $attribute } doit être un numéro de carte bancaire
validation-date = { $attribute } doit être une date ISO 8601 valide
validation-date_format = { $attribute } doit être une date valide au format { $format }
validation-email = { $attribute } doit être une adresse e-mail valide
validation-ends_with = { $attribute } doit se terminer par { $values }
validation-exists = { $attribute } n'existe pas
validation-html-attributes = { $attribute } contient trop d'attributs sur un élément, { $max } au maximum
validation-html-depth = { $attribute } s'imbrique sur plus de { $max } niveaux
validation-html-input = { $attribute } dépasse { $max } octets
validation-html-nodes = { $attribute } contient trop de balises
validation-in = { $attribute } doit être l'une des valeurs suivantes : { $values }
validation-integer = { $attribute } doit être un nombre entier
validation-invalid = { $attribute } n'est pas valide
validation-ip = { $attribute } doit être une adresse IP
validation-ipv4 = { $attribute } doit être une adresse IPv4
validation-ipv6 = { $attribute } doit être une adresse IPv6
validation-json = { $attribute } doit être une chaîne JSON valide
validation-locale = { $attribute } doit être une locale valide
validation-max-array = { $attribute } ne doit pas contenir plus de { $max } { $max ->
        [one] élément
       *[other] éléments
    }
validation-max-numeric = { $attribute } ne doit pas être supérieur à { $max }
validation-max-string = { $attribute } ne doit pas dépasser { $max } { $max ->
        [one] caractère
       *[other] caractères
    }
validation-min-array = { $attribute } doit contenir au moins { $min } { $min ->
        [one] élément
       *[other] éléments
    }
validation-min-numeric = { $attribute } ne doit pas être inférieur à { $min }
validation-min-string = { $attribute } doit contenir au moins { $min } { $min ->
        [one] caractère
       *[other] caractères
    }
validation-numeric = { $attribute } doit être un nombre
validation-object = { $attribute } doit être un objet
validation-phone = { $attribute } doit être un numéro de téléphone valide
validation-prohibited = la propriété { $attribute } ne doit pas exister
validation-regex = { $attribute } doit correspondre à l'expression régulière { $pattern }
validation-required = { $attribute } ne doit pas être vide
validation-same = { $attribute } doit correspondre à { $other }
validation-starts_with = { $attribute } doit commencer par { $values }
validation-string = { $attribute } doit être une chaîne de caractères
validation-unicode_name = { $attribute } ne doit contenir que des lettres, des chiffres, des espaces, des traits d'union, des apostrophes et des points
validation-unique = { $attribute } est déjà utilisé
validation-url = { $attribute } doit être une adresse URL
validation-uuid = { $attribute } doit être un UUID
