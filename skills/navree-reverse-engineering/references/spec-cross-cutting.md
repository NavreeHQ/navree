# Template: cross-cutting concerns

`05-cross/CROSS_CUTTING.md` covers what every module shares: how people
sign in, what is audited, what the product sends, the settings people
change, files, search, languages and time zones. Each has its own section
with the same row columns. Skip none: write `none` with a reason when the
old product has no such thing.

## Format

```text
## Authentication
| ID | Item | Old behaviour | New behaviour | Source | Known by | Conf. | Status |
Items: sign-in methods (password, single sign-on, social, API keys), password storage
(the hash scheme, for migrating users), session length, sign-out everywhere,
two-factor, lockout and throttling, password reset, invitations, service accounts.

## Audit
| ID | Event | Recorded today | Fields kept | Who reads it | Retention | Source | Known by | Conf. | Status |

## Notifications
| ID | Notification | Trigger | Recipients | Channels | Template and language | Opt-out | Source | Known by | Conf. | Status |

## Settings
| ID | Setting | Level (product, organization, tenant, user) | Type | Default | Who changes it | Source | Known by | Conf. | Status |

## Files
| ID | File kind | Attached to | Types and size | Where stored today | Who reads it | Source | Known by | Conf. | Status |

## Search
| ID | Search | Over | Fields | Match | Scope | Source | Known by | Conf. | Status |

## Languages and formats
| ID | Item | Old behaviour | New behaviour | Source | Known by | Conf. | Status |
Items: languages offered, where texts live, right-to-left, number, date and money formats,
translated data (product names per language).

## Time zones
| ID | Item | Old behaviour | New behaviour | Source | Known by | Conf. | Status |
Items: the zone the server and database ran in, how dates were stored (local or UTC),
the zone per tenant and per user, business days and holidays.
```

## What to look for

- **Authentication**: the password hash scheme decides whether users keep
  their passwords after migration. Navree's sign-in stores and checks
  Argon2id hashes only. Users with another scheme (bcrypt, salted SHA,
  md5) either reset their password on first visit, or the app checks the
  old hash in its own code once and stores an Argon2id hash: a task to
  build, not a feature. Plain or reversible passwords always mean a reset.
  The owner chooses.
- **Audit**: old products keep partial logs (a `modified_by` column, a log
  table written by some screens). List what exists, then what the owner
  needs. In the new product, model writes through the unit of work record
  change events, which the audit trail keeps along with sign-ins,
  permission changes and refusals; say which actions also need an audit
  entry of their own
  (exports, reveals of personal data, permission changes).
- **Notifications**: every mail, SMS and in-app message, with its trigger
  and recipients. The mail catcher used at intake shows what the old
  product sends; compare with what the code says it sends.
- **Settings**: values in a settings table, in config files, in constants.
  A setting a tenant changes is a tenant setting; a constant nobody
  changed in years may be code.
- **Files**: where the old product keeps uploads (a folder on the web
  server, a bucket, blobs in the database), how they are named, and who may
  download them. Files on the web server's public path are a finding.
- **Search**: what the search boxes really search, and whether they respect
  tenant and role scope.
- **Time zones**: find the old server's zone (`SELECT @@time_zone`,
  `SHOW timezone`, the PHP or app config). Dates stored as local time with
  no zone must be converted during migration with the zone they were
  written in.

## Worked example

```text
## Authentication
| cross.1 | Password storage | md5 without salt for users before 2019, bcrypt after | md5 users get a reset mail; bcrypt users: reset too, or an app check of the old hash at first sign-in (a task) | SQL-060 (812 md5, 1 304 bcrypt) | data | high | question Q-020 |
| cross.2 | Lockout | none | throttled sign-in; two-factor for owners and accountants | login.php | code | high | confirmed |

## Notifications
| cross.10 | Invoice mail | invoice sent | customer billing email | mail | invoice_mail.html, English only | none (transactional) | mail catcher, CAP-080 | running | high | confirmed |

## Time zones
| cross.40 | Server zone | MySQL SYSTEM = Europe/London; dates stored as local time | stored as UTC, shown in the branch's zone | SQL-061 | data | high | confirmed |
```

## Checks before the file is done

- Every section is filled in or `none` with a reason.
- Every `NT`, `FL` and `ST` map row is owned by a row here or in a module.
- The password hash scheme and the old server's time zone are known, not
  guessed.
- The file ends with `## Questions`, `## Deliberate changes` and
  `## New map rows` (formats in `spec-module.md`); an empty one says
  "none".
