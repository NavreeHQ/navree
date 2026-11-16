# Third-party notices

## Leaflet

`assets/map.js` holds Leaflet 1.9.4, <https://leafletjs.com>, bundled with this plugin's own
script by `assets/build.mjs` from Leaflet's module build (`dist/leaflet-src.esm.js`), so the
script writes no global. `assets/map.css` holds Leaflet's stylesheet from the same release, with
the three rules that name pictures (`url(images/...)`) and the one rule for VML
(`behavior: url(#default#VML)`) taken out, followed by this plugin's own rules. Leaflet is
licensed under the BSD 2-Clause licence:

```
BSD 2-Clause License

Copyright (c) 2010-2023, Vladimir Agafonkin
Copyright (c) 2010-2011, CloudMade
All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

   1. Redistributions of source code must retain the above copyright notice, this list of
      conditions and the following disclaimer.

   2. Redistributions in binary form must reproduce the above copyright notice, this list of
      conditions and the following disclaimer in the documentation and/or other materials
      provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

**Changes.** None to Leaflet's code. The build leaves out the four stylesheet rules named above:
this plugin's map draws a circle for its pin and has no layers control, no browser of today reads
the VML rule, and a plugin's stylesheet may name no address but a `data:` one. The
`legalComments: "eof"` option of the bundler keeps Leaflet's own notice comments at the end of
`map.js`.

The glue that calls Leaflet (`assets/src/map.js`) and the rules in `assets/src/own.css` are this
plugin's own.
