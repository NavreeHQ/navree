# Validation messages in Italian, shipped with navree-i18n.
#
# Each id is `validation-` and a rule key with dots as hyphens
# (`min.string` is `validation-min-string`). Variables: $attribute (the
# field's display name, from `field-<path>`), and the rule's own: $min,
# $max, $other, $values, $value, $format, $pattern.

validation-alpha_num = { $attribute } deve contenere solo lettere e numeri
validation-array = { $attribute } deve essere un array
validation-ascii = { $attribute } deve contenere solo caratteri ASCII
validation-boolean = { $attribute } deve essere un valore booleano
validation-confirmed = la conferma di { $attribute } non corrisponde
validation-contains = { $attribute } deve contenere la stringa { $value }
validation-credit_card = { $attribute } deve essere una carta di credito
validation-date = { $attribute } deve essere una data ISO 8601 valida
validation-date_format = { $attribute } deve essere una data valida nel formato { $format }
validation-email = { $attribute } deve essere un indirizzo email valido
validation-ends_with = { $attribute } deve terminare con { $values }
validation-exists = { $attribute } non esiste
validation-in = { $attribute } deve essere uno dei seguenti valori: { $values }
validation-integer = { $attribute } deve essere un numero intero
validation-invalid = { $attribute } non è valido
validation-ip = { $attribute } deve essere un indirizzo IP
validation-ipv4 = { $attribute } deve essere un indirizzo IPv4
validation-ipv6 = { $attribute } deve essere un indirizzo IPv6
validation-json = { $attribute } deve essere una stringa JSON valida
validation-max-array = { $attribute } non deve contenere più di { $max } { $max ->
        [one] elemento
       *[other] elementi
    }
validation-max-numeric = { $attribute } non deve essere maggiore di { $max }
validation-max-string = { $attribute } non deve superare { $max } { $max ->
        [one] carattere
       *[other] caratteri
    }
validation-min-array = { $attribute } deve contenere almeno { $min } { $min ->
        [one] elemento
       *[other] elementi
    }
validation-min-numeric = { $attribute } non deve essere minore di { $min }
validation-min-string = { $attribute } deve contenere almeno { $min } { $min ->
        [one] carattere
       *[other] caratteri
    }
validation-numeric = { $attribute } deve essere un numero
validation-object = { $attribute } deve essere un oggetto
validation-phone = { $attribute } deve essere un numero di telefono valido
validation-prohibited = la proprietà { $attribute } non deve esistere
validation-regex = { $attribute } deve corrispondere all'espressione regolare { $pattern }
validation-required = { $attribute } non deve essere vuoto
validation-same = { $attribute } deve corrispondere a { $other }
validation-starts_with = { $attribute } deve iniziare con { $values }
validation-string = { $attribute } deve essere una stringa
validation-unique = { $attribute } è già in uso
validation-url = { $attribute } deve essere un indirizzo URL
validation-uuid = { $attribute } deve essere un UUID
