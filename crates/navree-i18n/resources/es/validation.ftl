# Validation messages in Spanish, shipped with navree-i18n.
#
# Each id is `validation-` and a rule key with dots as hyphens
# (`min.string` is `validation-min-string`). Variables: $attribute (the
# field's display name, from `field-<path>`), and the rule's own: $min,
# $max, $other, $values, $value, $format, $pattern.

validation-alpha_num = { $attribute } solo debe contener letras y números
validation-array = { $attribute } debe ser una lista
validation-ascii = { $attribute } solo debe contener caracteres ASCII
validation-boolean = { $attribute } debe ser un valor booleano
validation-confirmed = la confirmación de { $attribute } no coincide
validation-contains = { $attribute } debe contener la cadena { $value }
validation-country = { $attribute } debe ser un código de país ISO 3166-1 alfa-2 válido
validation-credit_card = { $attribute } debe ser una tarjeta de crédito
validation-date = { $attribute } debe ser una fecha ISO 8601 válida
validation-date_format = { $attribute } debe ser una fecha válida con el formato { $format }
validation-email = { $attribute } debe ser una dirección de correo electrónico válida
validation-ends_with = { $attribute } debe terminar con { $values }
validation-exists = { $attribute } no existe
validation-in = { $attribute } debe ser uno de los siguientes valores: { $values }
validation-integer = { $attribute } debe ser un número entero
validation-invalid = { $attribute } no es válido
validation-ip = { $attribute } debe ser una dirección IP
validation-ipv4 = { $attribute } debe ser una dirección IPv4
validation-ipv6 = { $attribute } debe ser una dirección IPv6
validation-json = { $attribute } debe ser una cadena JSON válida
validation-locale = { $attribute } debe ser una configuración regional válida
validation-max-array = { $attribute } no debe contener más de { $max } { $max ->
        [one] elemento
       *[other] elementos
    }
validation-max-numeric = { $attribute } no debe ser mayor que { $max }
validation-max-string = { $attribute } no debe tener más de { $max } { $max ->
        [one] carácter
       *[other] caracteres
    }
validation-min-array = { $attribute } debe contener al menos { $min } { $min ->
        [one] elemento
       *[other] elementos
    }
validation-min-numeric = { $attribute } no debe ser menor que { $min }
validation-min-string = { $attribute } debe tener al menos { $min } { $min ->
        [one] carácter
       *[other] caracteres
    }
validation-numeric = { $attribute } debe ser un número
validation-object = { $attribute } debe ser un objeto
validation-phone = { $attribute } debe ser un número de teléfono válido
validation-prohibited = la propiedad { $attribute } no debe existir
validation-regex = { $attribute } debe coincidir con la expresión regular { $pattern }
validation-required = { $attribute } no debe estar vacío
validation-same = { $attribute } debe coincidir con { $other }
validation-starts_with = { $attribute } debe comenzar con { $values }
validation-string = { $attribute } debe ser una cadena de texto
validation-unicode_name = { $attribute } solo puede contener letras, números, espacios, guiones, apóstrofos y puntos
validation-unique = { $attribute } ya está en uso
validation-url = { $attribute } debe ser una dirección URL
validation-uuid = { $attribute } debe ser un UUID
