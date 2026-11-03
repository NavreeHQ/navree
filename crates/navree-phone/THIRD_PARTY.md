# Third-party notices

## libphonenumber's numbering plans

`src/data.rs` holds the numbering plans of libphonenumber, release v9.0.42: the file
`resources/PhoneNumberMetadata.xml` of <https://github.com/google/libphonenumber>, SHA-256
`b6491939ddaeb9d585a34fe0f0ebd096e83c5ac9a1c5d6dddf38ba4b162b28ca`. The same data, in
another form, is in `crates/navree-ui/runtime/src/phone-data.json` and the browser asset
built from it.

Copyright (C) 2009 The Libphonenumber Authors.

Licensed under the Apache License, Version 2.0 (the "License"); you may not use this file
except in compliance with the License. You may obtain a copy of the License at
<http://www.apache.org/licenses/LICENSE-2.0>, and a copy is in this repository's
`LICENSE-APACHE`.

Unless required by applicable law or agreed to in writing, software distributed under the
License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
either express or implied. See the License for the specific language governing permissions
and limitations under the License.

**Changes.** `tools/metadata.mjs` reads the file and writes it in a new form: each
pattern compiled into a small instruction set over the digits 0 to 9, whitespace removed,
the lengths of each territory's number types joined into its general lengths, the national
prefix formatting rules applied to each format, the English territory names taken from the
file's comments, and everything packed into tables. No number, pattern or rule is changed.

**NOTICE.** The release has no `NOTICE` file, so there is none to carry.

The parser, the pattern matcher and the formatter that read these tables are this crate's
own code; none of libphonenumber's code is copied or translated.
