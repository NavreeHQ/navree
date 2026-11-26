# AWS Signature Version 4 test vectors

`vectors.json` holds published AWS Signature Version 4 (`AWS4-HMAC-SHA256`) test vectors from two upstream sources:

1. **AWS Signature Version 4 Test Suite** (`suite_vectors`): request/signature pairs published by Amazon Web Services in the AWS General Reference (`example.amazonaws.com`, region `us-east-1`, timestamp `20150830T123600Z`), covering query ordering, duplicate headers, header trimming, form bodies and S3 single-pass path encoding (`get-utf8`, `get-space`).
2. **Amazon S3 REST API Authenticating Requests Examples** (`s3_header_vectors` and `s3_presigned_vectors`): worked S3 signing examples published in the Amazon S3 API Reference (`Authenticating Requests: Using the Authorization Header` and `Authenticating Requests: Using Query Parameters`, `examplebucket.s3.amazonaws.com`, region `us-east-1`, timestamp `20130524T000000Z`), plus a port-bearing S3 `ListObjectsV2` vector (`localhost:9000`).

## Licence and attribution

Copyright Amazon.com, Inc. or its affiliates. Distributed by AWS in the AWS Signature Version 4 Test Suite and AWS SDK documentation under the Apache License, Version 2.0 (`http://www.apache.org/licenses/LICENSE-2.0`).
