fn clients() {
    // ruleid: navree-tls-verification-disabled
    let _ = reqwest::Client::builder().danger_accept_invalid_certs(true).build();
    // ruleid: navree-tls-verification-disabled
    let _ = reqwest::Client::builder().danger_accept_invalid_hostnames(true).build();
    // ruleid: navree-tls-verification-disabled
    let _ = rustls::ClientConfig::builder().dangerous().with_custom_certificate_verifier(v);
    // ok: navree-tls-verification-disabled
    let _ = reqwest::Client::builder().use_rustls_tls().build();
}
