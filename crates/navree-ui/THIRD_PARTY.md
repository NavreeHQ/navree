# Third-party notices

## shadcn/ui

`styles/*.css`, `src/components/*`, `src/select/*` and `src/pages/*` follow the structure of shadcn/ui's style layer:
slot names, variant names and component anatomy. The values are Navree's own.

MIT License. Copyright (c) 2023 shadcn.

Permission is hereby granted, free of charge, to any person obtaining a copy of this
software and associated documentation files (the "Software"), to deal in the Software
without restriction, including without limitation the rights to use, copy, modify,
merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice shall be included in all copies
or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE
OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## Unicode CLDR

`src/dates/data.rs` takes the first day of the week for each region from the Unicode CLDR's week
data. The names of months and weekdays, the order of a typed date and the words on the controls
are written for Navree.

Unicode License v3. Copyright (c) 1991-2025 Unicode, Inc. Data files and software are provided
"AS IS", without warranty of any kind; see https://www.unicode.org/license.txt.

## WAI-ARIA Authoring Practices

The date grid's keys and roles follow the ARIA Authoring Practices Guide's date picker dialog and
grid patterns (W3C Software and Document License). The code is Navree's own.

## Lucide

`src/icons/lucide.rs` holds the shapes of every icon in lucide-static 1.67.0 (2,019
icons). `icons.lock.json` pins the release by its SHA-512, and `runtime/icons.mjs`
regenerates the file from it.

ISC License. Copyright (c) 2026 Lucide Icons and Contributors. Permission to use, copy,
modify, and/or distribute this software for any purpose with or without fee is hereby
granted, provided that the above copyright notice and this permission notice appear in
all copies. THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR
CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR
PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING
OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

Icons derived from the Feather project are under the MIT License, Copyright (c)
2013-present Cole Bemis, with the same terms as the MIT text above. Lucide's LICENSE
lists them.

## flag-icons

`src/flags/data.rs` holds the artwork of the 270 flags in flag-icons 7.6.0 (the 4 by 3
and the square set), cleaned by `runtime/icons.mjs`: ids made unique per flag, styles
and classes removed, nothing else changed. `icons.lock.json` pins the release by its
SHA-512.

MIT License. Copyright (c) 2013 Panayiotis Lipiridis.

Permission is hereby granted, free of charge, to any person obtaining a copy of this
software and associated documentation files (the "Software"), to deal in the Software
without restriction, including without limitation the rights to use, copy, modify,
merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice shall be included in all copies
or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE
OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## Unicode CLDR

The English names of the flags (`src/flags/data.rs`, from `main/en/territories.json` of
cldr-localenames-full 49.0.0 and `subdivisions/en/en.json` of cldr-subdivisions-full
49.0.0) are data from the Unicode Common Locale Data Repository. The names of six
places and bodies CLDR does not name (ASEAN, CEFTA, EAC, the League of Arab States, the
Pacific Community, Saint Helena) come from flag-icons' country.json.

Unicode License v3. Copyright (c) 2001-2026 Unicode, Inc. Permission is hereby granted,
free of charge, to any person obtaining a copy of data files and any associated
documentation (the "Data Files") or software and any associated documentation (the
"Software") to deal in the Data Files or Software without restriction, including without
limitation the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the Data Files or
Software are furnished to do so, provided that either (a) this copyright and permission
notice appear with all copies of the Data Files or Software, or (b) this copyright and
permission notice appear in associated Documentation. THE DATA FILES AND SOFTWARE ARE
PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT OF THIRD PARTY RIGHTS. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS
INCLUDED IN THIS NOTICE BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL
DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER
IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA FILES OR SOFTWARE. Except as contained
in this notice, the name of a copyright holder shall not be used in advertising or
otherwise to promote the sale, use or other dealings in these Data Files or Software
without prior written authorization of the copyright holder.

## WAI-ARIA Authoring Practices

The keyboard behaviour of tabs, the menu button, menus and submenus, radio groups, toggle
buttons, alerts and the window splitter follows the W3C's WAI-ARIA Authoring Practices
Guide (https://www.w3.org/WAI/ARIA/apg/). The behaviour is followed; the code is Navree's own.

## Geist

`assets/fonts/geist.woff2` is under the SIL Open Font License 1.1:
`assets/fonts/LICENSE-geist.txt`.

## libphonenumber's numbering plans

`runtime/src/phone-data.json`, and the `phone-data` asset built from it, hold the numbering
plans of libphonenumber, release v9.0.42, under the Apache License 2.0, Copyright (C) 2009
The Libphonenumber Authors. `crates/navree-phone/THIRD_PARTY.md` has the notice, the
checksum of the source file and what was changed.
