# Validation messages in Brazilian Portuguese, shipped with navree-i18n.
#
# Each id is `validation-` and a rule key with dots as hyphens
# (`min.string` is `validation-min-string`). Variables: $attribute (the
# field's display name, from `field-<path>`), and the rule's own: $min,
# $max, $other, $values, $value, $format, $pattern.

validation-alpha_num = { $attribute } deve conter apenas letras e números
validation-array = { $attribute } deve ser uma lista
validation-ascii = { $attribute } deve conter apenas caracteres ASCII
validation-boolean = { $attribute } deve ser um valor booleano
validation-confirmed = a confirmação de { $attribute } não corresponde
validation-contains = { $attribute } deve conter o texto { $value }
validation-country = { $attribute } deve ser um código de país ISO 3166-1 alfa-2 válido
validation-credit_card = { $attribute } deve ser um cartão de crédito
validation-date = { $attribute } deve ser uma data ISO 8601 válida
validation-date_format = { $attribute } deve ser uma data válida no formato { $format }
validation-email = { $attribute } deve ser um endereço de e-mail válido
validation-ends_with = { $attribute } deve terminar com { $values }
validation-exists = { $attribute } não existe
validation-in = { $attribute } deve ser um dos seguintes valores: { $values }
validation-integer = { $attribute } deve ser um número inteiro
validation-invalid = { $attribute } é inválido
validation-ip = { $attribute } deve ser um endereço IP
validation-ipv4 = { $attribute } deve ser um endereço IPv4
validation-ipv6 = { $attribute } deve ser um endereço IPv6
validation-json = { $attribute } deve ser um texto JSON válido
validation-locale = { $attribute } deve ser uma localidade válida
validation-max-array = { $attribute } não deve conter mais de { $max } { $max ->
        [one] item
       *[other] itens
    }
validation-max-numeric = { $attribute } não deve ser maior que { $max }
validation-max-string = { $attribute } não deve ter mais de { $max } { $max ->
        [one] caractere
       *[other] caracteres
    }
validation-min-array = { $attribute } deve conter pelo menos { $min } { $min ->
        [one] item
       *[other] itens
    }
validation-min-numeric = { $attribute } não deve ser menor que { $min }
validation-min-string = { $attribute } deve ter pelo menos { $min } { $min ->
        [one] caractere
       *[other] caracteres
    }
validation-numeric = { $attribute } deve ser um número
validation-object = { $attribute } deve ser um objeto
validation-phone = { $attribute } deve ser um número de telefone válido
validation-prohibited = a propriedade { $attribute } não deve existir
validation-regex = { $attribute } deve corresponder à expressão regular { $pattern }
validation-required = { $attribute } não deve estar vazio
validation-same = { $attribute } deve corresponder a { $other }
validation-starts_with = { $attribute } deve começar com { $values }
validation-string = { $attribute } deve ser um texto
validation-unicode_name = { $attribute } deve conter apenas letras, números, espaços, hifens, apóstrofos e pontos
validation-unique = { $attribute } já está em uso
validation-url = { $attribute } deve ser um endereço URL
validation-uuid = { $attribute } deve ser um UUID
