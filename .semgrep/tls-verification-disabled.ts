import https from "node:https";

// ruleid: navree-tls-verification-disabled-js
const insecure = new https.Agent({ rejectUnauthorized: false });

// ruleid: navree-tls-verification-disabled-js
process.env.NODE_TLS_REJECT_UNAUTHORIZED = "0";

// ok: navree-tls-verification-disabled-js
const secure = new https.Agent({ rejectUnauthorized: true, ca: "pem" });

export { insecure, secure };
