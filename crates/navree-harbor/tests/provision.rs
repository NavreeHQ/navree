//! The provisioning scripts and the files they install, as snapshots.

mod common;

use common::{MULTI, SINGLE, assert_bash_syntax, config, plain};
use navree_harbor::layout::Slot;
use navree_harbor::provision::{Facts, Passwords, Step, plan};
use navree_harbor::render::{AppInfo, FrontendInfo, site};

fn facts(memory_mb: u64) -> Facts {
    Facts {
        id: "ubuntu".into(),
        version: "24.04".into(),
        arch: "x86_64".into(),
        memory_mb,
    }
}

fn passwords() -> Passwords {
    Passwords {
        postgres: Some("PgPassword123".into()),
        redis: Some("RedisPassword456".into()),
    }
}

const KEYS: &[&str] = &[
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMine me@laptop",
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICi ci-deploy",
];

fn keys() -> Vec<String> {
    KEYS.iter().map(|k| (*k).to_owned()).collect()
}

fn steps(text: &str, server: &str, info: &AppInfo, memory_mb: u64) -> Vec<Step> {
    let config = config(text);
    let server = config.server(server).unwrap().clone();
    plan(
        &config,
        &server,
        info,
        &facts(memory_mb),
        &passwords(),
        &keys(),
    )
    .unwrap()
}

fn step<'a>(steps: &'a [Step], area: &str) -> &'a Step {
    steps
        .iter()
        .find(|s| s.area == area)
        .unwrap_or_else(|| panic!("no {area} step"))
}

#[test]
fn a_single_server_gets_every_area_in_order() {
    let steps = steps(SINGLE, "app-1", &plain(), 1987);
    let areas: Vec<&str> = steps.iter().map(|s| s.area).collect();
    assert_eq!(
        areas,
        [
            "base", "users", "ssh", "firewall", "postgres", "redis", "caddy", "systemd"
        ]
    );
    let summaries: Vec<String> = steps
        .iter()
        .map(|s| format!("{:<9} {}", s.area, s.summary))
        .collect();
    insta::assert_snapshot!(summaries.join("\n"), @r"
    base      updates, unattended upgrades, chrony, 2 GB swap, UTC
    users     navree, with 2 SSH keys
    ssh       no passwords, root by key only, host key checked
    firewall  ufw: 22/tcp, 80/tcp, 443/tcp, 443/udp; fail2ban
    postgres  PostgreSQL 17, database crm_api
    redis     Redis on 127.0.0.1, 256 MB
    caddy     Caddy 2, site crm.example.com
    systemd   crm-api-web@blue, @green, crm-api-worker@1, @2, crm-api-scheduler, crm-api-backup.timer
    ");
    for step in &steps {
        assert_bash_syntax(step.area, &step.script);
    }
}

#[test]
fn base_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "base").script);
}

#[test]
fn base_script_without_swap_on_a_large_server() {
    let steps = steps(SINGLE, "app-1", &plain(), 8000);
    let script = &step(&steps, "base").script;
    assert!(!script.contains("swapfile"), "{script}");
}

#[test]
fn users_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "users").script);
}

#[test]
fn ssh_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "ssh").script);
}

#[test]
fn firewall_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "firewall").script);
}

#[test]
fn postgres_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "postgres").script);
}

#[test]
fn redis_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "redis").script);
}

#[test]
fn caddy_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "caddy").script);
}

#[test]
fn systemd_script() {
    insta::assert_snapshot!(step(&steps(SINGLE, "app-1", &plain(), 1987), "systemd").script);
}

#[test]
fn the_caddy_site_follows_the_live_slot() {
    let config = config(SINGLE);
    let server = config.servers[0].clone();
    let blue = site(&config, &server, &plain(), Slot::Blue).unwrap();
    insta::assert_snapshot!(blue, @r"
    # crm-api: written by navree harbor:provision; navree deploy points the
    # reverse_proxy lines at the live slot (blue).
    crm.example.com {
    	encode zstd gzip

    	@metrics path /metrics
    	respond @metrics 404

    	reverse_proxy 127.0.0.1:8081
    }

    # Internal entry for workers and the scheduler, always pointing at the live slot.
    http://:8079 {
    	bind 127.0.0.1
    	reverse_proxy 127.0.0.1:8081
    }
    ");
    let green = site(&config, &server, &plain(), Slot::Green).unwrap();
    assert!(green.contains("reverse_proxy 127.0.0.1:8082"));
    assert!(!green.contains("8081"));
    let grpc = AppInfo {
        h2c: true,
        ..plain()
    };
    let h2c = site(&config, &server, &grpc, Slot::Blue).unwrap();
    assert!(h2c.contains("reverse_proxy h2c://127.0.0.1:8081"), "{h2c}");
}

#[test]
fn custom_domains_get_on_demand_tls_after_the_apps_ask_endpoint() {
    let text = format!("{SINGLE}\n[web.custom_domains]\nenabled = true\n");
    let harbor = config(&text);
    let server = harbor.servers[0].clone();
    insta::assert_snapshot!(site(&harbor, &server, &plain(), Slot::Blue).unwrap(), @r"
    # crm-api: written by navree harbor:provision; navree deploy points the
    # reverse_proxy lines at the live slot (blue).
    crm.example.com {
    	encode zstd gzip

    	@metrics path /metrics
    	respond @metrics 404

    	reverse_proxy 127.0.0.1:8081
    }

    # Workspaces' own domains: Caddy gets each one's certificate on its first
    # visit, once /_domains/allow allows it.
    https:// {
    	tls {
    		on_demand
    	}
    	encode zstd gzip

    	@metrics path /metrics
    	respond @metrics 404

    	reverse_proxy 127.0.0.1:8081
    }

    # Internal entry for workers and the scheduler, always pointing at the live slot.
    http://:8079 {
    	bind 127.0.0.1
    	reverse_proxy 127.0.0.1:8081
    }
    ");
    let planned = steps(&text, "app-1", &plain(), 1987);
    let caddy = &step(&planned, "caddy").script;
    insta::assert_snapshot!("custom_domains_caddy_script", caddy);
    assert!(
        caddy.contains("on_demand_tls {\n\t\task http://127.0.0.1:8079/_domains/allow\n\t}"),
        "{caddy}"
    );
    assert_bash_syntax("caddy", caddy);

    // With a front end, the domains can go to it, and the ask path follows
    // the app's `domains.ask_path`.
    let text = format!(
        "{SINGLE}\n[frontend]\npath = \"web\"\n\n[web.custom_domains]\nenabled = true\nask_path = \"/caddy/ask\"\nto = \"frontend\"\n"
    );
    let harbor = config(&text);
    let server = harbor.servers[0].clone();
    let green = site(&harbor, &server, &frontend(), Slot::Green).unwrap();
    let custom = green.split("https:// {").nth(1).unwrap();
    assert!(custom.contains("reverse_proxy 127.0.0.1:3002"), "{green}");
    assert!(green.contains("once /caddy/ask allows it"), "{green}");
    let planned = steps(&text, "app-1", &frontend(), 1987);
    assert!(
        step(&planned, "caddy")
            .script
            .contains("ask http://127.0.0.1:8079/caddy/ask"),
    );
}

#[test]
fn the_proxy_protocol_and_custom_domains_share_the_global_options() {
    let planned = steps(
        &format!("{MULTI}\n[web.custom_domains]\nenabled = true\n"),
        "web-1",
        &plain(),
        4096,
    );
    let caddy = &step(&planned, "caddy").script;
    let global = caddy
        .split("import /etc/caddy/sites/*.caddy")
        .next()
        .unwrap();
    assert!(global.contains("on_demand_tls {"), "{caddy}");
    assert!(global.contains("listener_wrappers {"), "{caddy}");
    assert_eq!(global.matches("\n{\n").count(), 1, "{caddy}");
}

/// Behind an HTTP load balancer (no PROXY protocol) Caddy would replace
/// `X-Forwarded-For` with the balancer's address, and every client would
/// share one address and one rate-limit budget. `deploy.trusted_proxies`
/// is what Caddy's `trusted_proxies` takes, so it keeps the header.
#[test]
fn caddy_trusts_the_load_balancers_forwarded_for() {
    let base = SINGLE.replace(
        "[deploy]\n",
        "[deploy]\ntrusted_proxies = [\"10.0.0.0/24\", \"2001:db8:aa::/48\", \"192.0.2.7\"]\n",
    );
    let balancer = |extra: &str| format!("{base}{extra}");
    let planned = steps(&balancer(""), "app-1", &plain(), 1987);
    let caddy = &step(&planned, "caddy").script;
    insta::assert_snapshot!("http_load_balancer_caddy_script", caddy);
    let global = caddy
        .split("import /etc/caddy/sites/*.caddy")
        .next()
        .unwrap();
    assert!(
        global.contains(
            "\tservers {\n\t\ttrusted_proxies static 10.0.0.0/24 2001:db8:aa::/48 192.0.2.7\n\t}\n}"
        ),
        "{caddy}"
    );
    assert!(!global.contains("proxy_protocol"), "{caddy}");
    assert_bash_syntax("caddy", caddy);

    // Custom domains share the one global options block.
    let planned = steps(
        &balancer("\n[web.custom_domains]\nenabled = true\n"),
        "app-1",
        &plain(),
        1987,
    );
    let global = step(&planned, "caddy")
        .script
        .split("import /etc/caddy/sites/*.caddy")
        .next()
        .unwrap()
        .to_owned();
    assert!(global.contains("on_demand_tls {"), "{global}");
    assert!(
        global.contains("trusted_proxies static 10.0.0.0/24"),
        "{global}"
    );
    assert_eq!(global.matches("\n{\n").count(), 1, "{global}");

    // Without trusted proxies Caddy keeps its defaults (it believes no
    // forwarded header), and with the PROXY protocol the client address is
    // already the connection's.
    let planned = steps(SINGLE, "app-1", &plain(), 1987);
    assert!(
        !step(&planned, "caddy").script.contains("trusted_proxies"),
        "{}",
        step(&planned, "caddy").script
    );
    let planned = steps(MULTI, "web-1", &plain(), 4096);
    assert!(
        !step(&planned, "caddy").script.contains("trusted_proxies"),
        "{}",
        step(&planned, "caddy").script
    );
}

fn frontend() -> AppInfo {
    AppInfo {
        h2c: false,
        frontend: Some(FrontendInfo {
            path: "web".into(),
            manager: "pnpm".into(),
            build: "pnpm install --frozen-lockfile && pnpm build".into(),
            command: "pnpm start".into(),
            health_check: "/".into(),
            ports: [3001, 3002],
            node_version: 24,
        }),
    }
}

#[test]
fn a_front_end_takes_the_domain_and_the_api_its_own() {
    let text = format!("{SINGLE}\n[frontend]\npath = \"web\"\n",).replace(
        "domain = \"crm.example.com\"",
        "domain = \"crm.example.com\"\napi_domain = \"api.crm.example.com\"",
    );
    let config = config(&text);
    let server = config.servers[0].clone();
    insta::assert_snapshot!(site(&config, &server, &frontend(), Slot::Green).unwrap(), @r"
    # crm-api: written by navree harbor:provision; navree deploy points the
    # reverse_proxy lines at the live slot (green).
    crm.example.com {
    	encode zstd gzip

    	reverse_proxy 127.0.0.1:3002
    }

    api.crm.example.com {
    	encode zstd gzip

    	@metrics path /metrics
    	respond @metrics 404

    	reverse_proxy 127.0.0.1:8082
    }

    # Internal entry for workers and the scheduler, always pointing at the live slot.
    http://:8079 {
    	bind 127.0.0.1
    	reverse_proxy 127.0.0.1:8082
    }
    ");
    let steps = plan(
        &config,
        &server,
        &frontend(),
        &facts(8000),
        &passwords(),
        &keys(),
    )
    .unwrap();
    let areas: Vec<&str> = steps.iter().map(|s| s.area).collect();
    assert_eq!(
        areas,
        [
            "base", "users", "ssh", "firewall", "postgres", "redis", "caddy", "node", "systemd"
        ]
    );
    insta::assert_snapshot!("node_script", step(&steps, "node").script);
    let systemd = &step(&steps, "systemd").script;
    assert!(systemd.contains("crm-api-frontend@.service"), "{systemd}");
    assert!(
        systemd.contains("ExecStart=/bin/sh -c 'exec pnpm start'"),
        "{systemd}"
    );
    let users = &step(&steps, "users").script;
    assert!(users.contains("/usr/bin/systemctl restart crm-api-frontend@green"));
    for step in &steps {
        assert_bash_syntax(step.area, &step.script);
    }
}

#[test]
fn several_servers_each_get_their_roles() {
    let areas = |server: &str| -> Vec<&'static str> {
        steps(MULTI, server, &plain(), 4096)
            .iter()
            .map(|s| s.area)
            .collect()
    };
    assert_eq!(
        areas("web-1"),
        ["base", "users", "ssh", "firewall", "caddy", "systemd"]
    );
    assert_eq!(
        areas("worker-1"),
        ["base", "users", "ssh", "firewall", "systemd"]
    );
    assert_eq!(
        areas("data-1"),
        [
            "base", "users", "ssh", "firewall", "postgres", "redis", "systemd"
        ]
    );
    for server in ["web-1", "web-2", "worker-1", "data-1"] {
        for step in steps(MULTI, server, &plain(), 4096) {
            assert_bash_syntax(step.area, &step.script);
        }
    }
}

#[test]
fn the_data_server_admits_the_app_servers_on_the_private_network() {
    let steps = steps(MULTI, "data-1", &plain(), 4096);
    let firewall = step(&steps, "firewall");
    assert_eq!(
        firewall.summary,
        "ufw: 22/tcp, 5432/tcp from 3 servers, 6379/tcp from 3 servers; fail2ban"
    );
    insta::assert_snapshot!("data_postgres_script", step(&steps, "postgres").script);
    let redis = step(&steps, "redis");
    assert_eq!(redis.summary, "Redis on 127.0.0.1 and 10.0.0.31, 256 MB");
    // Plain addresses: the `-` (optional address) prefix needs Redis 6.2,
    // and Ubuntu 22.04 ships 6.0.
    assert!(redis.script.contains("\nbind 127.0.0.1 10.0.0.31\n"));
}

#[test]
fn redis_binds_loopback_in_a_form_every_ubuntu_redis_reads() {
    let steps = steps(SINGLE, "app-1", &plain(), 1987);
    let redis = step(&steps, "redis");
    assert_eq!(redis.summary, "Redis on 127.0.0.1, 256 MB");
    assert!(
        redis.script.contains("\nbind 127.0.0.1\n"),
        "{}",
        redis.script
    );
    assert!(!redis.script.contains("-::1"));
}

#[test]
fn web_servers_behind_a_balancer() {
    let steps = steps(MULTI, "web-1", &plain(), 4096);
    insta::assert_snapshot!("multi_web_caddy_script", step(&steps, "caddy").script);
    let firewall = &step(&steps, "firewall").script;
    assert!(
        firewall.contains("allow from 10.0.0.21 to any port 8079 proto tcp comment 'harbor'"),
        "{firewall}"
    );
    let systemd = &step(&steps, "systemd").summary;
    assert_eq!(systemd, "crm-api-web@blue, @green, crm-api-scheduler");
}

#[test]
fn the_worker_server_runs_only_workers() {
    let steps = steps(MULTI, "worker-1", &plain(), 4096);
    insta::assert_snapshot!("worker_systemd_script", step(&steps, "systemd").script);
    let users = &step(&steps, "users").script;
    assert!(!users.contains("crm-api-web@"), "{users}");
    assert!(!users.contains("reload caddy"), "{users}");
}

#[test]
fn heredoc_breaking_keys_are_refused() {
    let config = config(SINGLE);
    let server = config.servers[0].clone();
    let error = plan(
        &config,
        &server,
        &plain(),
        &facts(1987),
        &passwords(),
        &["HARBOR_EOF".to_owned()],
    )
    .unwrap_err();
    assert!(error.to_string().contains("HARBOR_EOF"), "{error}");
}
