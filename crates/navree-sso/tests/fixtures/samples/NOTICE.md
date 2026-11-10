# Signed SAML samples

These responses come from the test data of two MIT-licensed SAML toolkits.
Real identity providers (AD FS, SimpleSAMLphp, OneLogin, Starfield) signed
them, so they check Navree's canonical form against signatures it did not
make. Most use RSA-SHA1 or 1024-bit keys, which Navree's verifier refuses;
the unit tests in `src/dsig.rs` check their digests and signatures with
`ring`'s legacy verifiers to prove the canonical form matches the signer's.

| File | Source | Original | What it is |
|---|---|---|---|
| `signed-response-sha1.xml` | python3-saml | `tests/data/responses/signed_message_response.xml.base64` | SimpleSAMLphp, response signed |
| `signed-assertion-sha1.xml` | python3-saml | `tests/data/responses/signed_assertion_response.xml.base64` | SimpleSAMLphp, assertion signed |
| `double-signed-sha1.xml` | python3-saml | `tests/data/responses/double_signed_response.xml.base64` | SimpleSAMLphp, both signed |
| `wrapped-response.xml` | python3-saml | `tests/data/responses/wrapped_response_2.xml.base64` | a signature wrapping attack |
| `inclusive-namespaces.xml` | ruby-saml | `test/responses/inclusive_namespaces.xml` | an `InclusiveNamespaces` prefix list |
| `inclusive-c14n.xml` | ruby-saml | `test/responses/starfield_response.xml.base64` | Canonical XML 1.0 (inclusive) |
| `ds-namespace-at-root.xml` | ruby-saml | `test/responses/response_with_ds_namespace_at_the_root.xml.base64` | the `ds` prefix declared on the response |
| `adfs-sha256.xml` | ruby-saml | `test/responses/adfs_response_sha256.xml` | AD FS, RSA-SHA256 (1024-bit key) |
| `adfs-sha512.xml` | ruby-saml | `test/responses/adfs_response_sha512.xml` | AD FS, RSA-SHA512 (1024-bit key) |
| `node-text-attack.xml` | ruby-saml | `test/responses/response_node_text_attack3.xml.base64` | a text node changed after signing |
| `assertion-wrapped.xml` | ruby-saml | `test/responses/response_assertion_wrapped.xml.base64` | an unsigned assertion before a signed one |

- python3-saml, https://github.com/SAML-Toolkits/python3-saml: Copyright (c)
  2010-2022 OneLogin, Inc.; Copyright (c) 2023 IAM Digital Services, SL.
- ruby-saml, https://github.com/SAML-Toolkits/ruby-saml: Copyright (c)
  2010-2022 OneLogin, Inc.; Copyright (c) 2023 IAM Digital Services, SL.

Both are distributed under the MIT License:

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
