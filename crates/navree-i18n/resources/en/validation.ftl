# Validation messages in English, shipped with navree-i18n.
#
# Each id is `validation-` and a rule key with dots as hyphens
# (`min.string` is `validation-min-string`). Variables: $attribute (the
# field's display name, from `field-<path>`), and the rule's own: $min,
# $max, $other, $values, $value, $format, $pattern.

validation-alpha_num = { $attribute } must contain only letters and numbers
validation-array = { $attribute } must be an array
validation-ascii = { $attribute } must contain only ASCII characters
validation-boolean = { $attribute } must be a boolean value
validation-confirmed = { $attribute } confirmation does not match
validation-contains = { $attribute } must contain a { $value } string
validation-country = { $attribute } must be a valid ISO 3166-1 alpha-2 country code
validation-credit_card = { $attribute } must be a credit card
validation-date = { $attribute } must be a valid ISO 8601 date string
validation-date_format = { $attribute } must be a valid date in the format { $format }
validation-email = { $attribute } must be a valid email address
validation-ends_with = { $attribute } must end with { $values }
validation-exists = { $attribute } does not exist
validation-html-attributes = { $attribute } has too many attributes on one element, at most { $max }
validation-html-depth = { $attribute } nests deeper than { $max } levels
validation-html-input = { $attribute } is longer than { $max } bytes
validation-html-nodes = { $attribute } has too much markup
validation-in = { $attribute } must be one of the following values: { $values }
validation-integer = { $attribute } must be an integer number
validation-invalid = { $attribute } is invalid
validation-ip = { $attribute } must be an ip address
validation-ipv4 = { $attribute } must be an IPv4 address
validation-ipv6 = { $attribute } must be an IPv6 address
validation-json = { $attribute } must be a valid JSON string
validation-locale = { $attribute } must be a valid locale
validation-max-array = { $attribute } must contain no more than { $max } elements
validation-max-numeric = { $attribute } must not be greater than { $max }
validation-max-string = { $attribute } must be shorter than or equal to { $max } characters
validation-min-array = { $attribute } must contain at least { $min } elements
validation-min-numeric = { $attribute } must not be less than { $min }
validation-min-string = { $attribute } must be longer than or equal to { $min } characters
validation-numeric = { $attribute } must be a number
validation-object = { $attribute } must be an object
validation-phone = { $attribute } must be a valid phone number
validation-prohibited = property { $attribute } should not exist
validation-regex = { $attribute } must match { $pattern } regular expression
validation-required = { $attribute } should not be empty
validation-same = { $attribute } must match { $other }
validation-starts_with = { $attribute } must start with { $values }
validation-string = { $attribute } must be a string
validation-unicode_name = { $attribute } must contain only letters, numbers, spaces, hyphens, apostrophes and periods
validation-unique = { $attribute } has already been taken
validation-url = { $attribute } must be a URL address
validation-uuid = { $attribute } must be a UUID
