# Validation messages in German, shipped with navree-i18n.
#
# Each id is `validation-` and a rule key with dots as hyphens
# (`min.string` is `validation-min-string`). Variables: $attribute (the
# field's display name, from `field-<path>`), and the rule's own: $min,
# $max, $other, $values, $value, $format, $pattern.

validation-alpha_num = { $attribute } darf nur Buchstaben und Ziffern enthalten
validation-array = { $attribute } muss ein Array sein
validation-ascii = { $attribute } darf nur ASCII-Zeichen enthalten
validation-boolean = { $attribute } muss ein Wahrheitswert sein
validation-confirmed = die Bestätigung von { $attribute } stimmt nicht überein
validation-contains = { $attribute } muss die Zeichenfolge { $value } enthalten
validation-country = { $attribute } muss ein gültiger Ländercode nach ISO 3166-1 alpha-2 sein
validation-credit_card = { $attribute } muss eine Kreditkartennummer sein
validation-date = { $attribute } muss ein gültiges ISO-8601-Datum sein
validation-date_format = { $attribute } muss ein gültiges Datum im Format { $format } sein
validation-email = { $attribute } muss eine gültige E-Mail-Adresse sein
validation-ends_with = { $attribute } muss mit { $values } enden
validation-exists = { $attribute } existiert nicht
validation-html-attributes = { $attribute } hat zu viele Attribute an einem Element, höchstens { $max }
validation-html-depth = { $attribute } ist tiefer als { $max } Ebenen verschachtelt
validation-html-input = { $attribute } ist länger als { $max } Bytes
validation-html-nodes = { $attribute } enthält zu viel Markup
validation-in = { $attribute } muss einer der folgenden Werte sein: { $values }
validation-integer = { $attribute } muss eine ganze Zahl sein
validation-invalid = { $attribute } ist ungültig
validation-ip = { $attribute } muss eine IP-Adresse sein
validation-ipv4 = { $attribute } muss eine IPv4-Adresse sein
validation-ipv6 = { $attribute } muss eine IPv6-Adresse sein
validation-json = { $attribute } muss eine gültige JSON-Zeichenfolge sein
validation-locale = { $attribute } muss eine gültige Locale sein
validation-max-array = { $attribute } darf höchstens { $max } { $max ->
        [one] Element
       *[other] Elemente
    } enthalten
validation-max-numeric = { $attribute } darf nicht größer als { $max } sein
validation-max-string = { $attribute } darf höchstens { $max } Zeichen lang sein
validation-min-array = { $attribute } muss mindestens { $min } { $min ->
        [one] Element
       *[other] Elemente
    } enthalten
validation-min-numeric = { $attribute } darf nicht kleiner als { $min } sein
validation-min-string = { $attribute } muss mindestens { $min } Zeichen lang sein
validation-numeric = { $attribute } muss eine Zahl sein
validation-object = { $attribute } muss ein Objekt sein
validation-phone = { $attribute } muss eine gültige Telefonnummer sein
validation-prohibited = die Eigenschaft { $attribute } darf nicht vorhanden sein
validation-regex = { $attribute } muss dem regulären Ausdruck { $pattern } entsprechen
validation-required = { $attribute } darf nicht leer sein
validation-same = { $attribute } muss mit { $other } übereinstimmen
validation-starts_with = { $attribute } muss mit { $values } beginnen
validation-string = { $attribute } muss eine Zeichenfolge sein
validation-unicode_name = { $attribute } darf nur Buchstaben, Ziffern, Leerzeichen, Bindestriche, Apostrophe und Punkte enthalten
validation-unique = { $attribute } ist bereits vergeben
validation-url = { $attribute } muss eine URL sein
validation-uuid = { $attribute } muss eine UUID sein
