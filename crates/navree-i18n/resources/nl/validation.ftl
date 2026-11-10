# Validation messages in Dutch, shipped with navree-i18n.
#
# Each id is `validation-` and a rule key with dots as hyphens
# (`min.string` is `validation-min-string`). Variables: $attribute (the
# field's display name, from `field-<path>`), and the rule's own: $min,
# $max, $other, $values, $value, $format, $pattern.

validation-alpha_num = { $attribute } mag alleen letters en cijfers bevatten
validation-array = { $attribute } moet een array zijn
validation-ascii = { $attribute } mag alleen ASCII-tekens bevatten
validation-boolean = { $attribute } moet een booleaanse waarde zijn
validation-confirmed = de bevestiging van { $attribute } komt niet overeen
validation-contains = { $attribute } moet de tekst { $value } bevatten
validation-country = { $attribute } moet een geldige ISO 3166-1 alpha-2-landcode zijn
validation-credit_card = { $attribute } moet een creditcardnummer zijn
validation-date = { $attribute } moet een geldige ISO 8601-datum zijn
validation-date_format = { $attribute } moet een geldige datum zijn in het formaat { $format }
validation-email = { $attribute } moet een geldig e-mailadres zijn
validation-ends_with = { $attribute } moet eindigen op { $values }
validation-exists = { $attribute } bestaat niet
validation-html-attributes = { $attribute } heeft te veel attributen op één element, maximaal { $max }
validation-html-depth = { $attribute } is dieper genest dan { $max } niveaus
validation-html-input = { $attribute } is langer dan { $max } bytes
validation-html-nodes = { $attribute } bevat te veel opmaak
validation-in = { $attribute } moet een van de volgende waarden zijn: { $values }
validation-integer = { $attribute } moet een geheel getal zijn
validation-invalid = { $attribute } is ongeldig
validation-ip = { $attribute } moet een IP-adres zijn
validation-ipv4 = { $attribute } moet een IPv4-adres zijn
validation-ipv6 = { $attribute } moet een IPv6-adres zijn
validation-json = { $attribute } moet een geldige JSON-tekenreeks zijn
validation-locale = { $attribute } moet een geldige taalcode zijn
validation-max-array = { $attribute } mag niet meer dan { $max } { $max ->
        [one] element
       *[other] elementen
    } bevatten
validation-max-numeric = { $attribute } mag niet groter zijn dan { $max }
validation-max-string = { $attribute } mag niet langer zijn dan { $max } { $max ->
        [one] teken
       *[other] tekens
    }
validation-min-array = { $attribute } moet minstens { $min } { $min ->
        [one] element
       *[other] elementen
    } bevatten
validation-min-numeric = { $attribute } mag niet kleiner zijn dan { $min }
validation-min-string = { $attribute } moet minstens { $min } { $min ->
        [one] teken
       *[other] tekens
    } lang zijn
validation-numeric = { $attribute } moet een getal zijn
validation-object = { $attribute } moet een object zijn
validation-phone = { $attribute } moet een geldig telefoonnummer zijn
validation-prohibited = de eigenschap { $attribute } mag niet bestaan
validation-regex = { $attribute } moet overeenkomen met de reguliere expressie { $pattern }
validation-required = { $attribute } mag niet leeg zijn
validation-same = { $attribute } moet overeenkomen met { $other }
validation-starts_with = { $attribute } moet beginnen met { $values }
validation-string = { $attribute } moet een tekenreeks zijn
validation-unicode_name = { $attribute } mag alleen letters, cijfers, spaties, koppeltekens, apostroffen en punten bevatten
validation-unique = { $attribute } is al in gebruik
validation-url = { $attribute } moet een URL zijn
validation-uuid = { $attribute } moet een UUID zijn
