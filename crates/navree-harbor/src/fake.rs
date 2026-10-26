//! [`FakeHost`]: a server kept in memory, for tests of the release state
//! machine (deploys, health-check failures, rollbacks, pruning) without
//! SSH.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use crate::config::{HarborConfig, Server};
use crate::env::EnvFile;
use crate::error::{HarborError, Result};
use crate::exec::Output;
use crate::host::{Health, Host, HostState, ReleaseInfo, SlotProcess};
use crate::layout::{Layout, Slot};

#[derive(Debug, Default, Clone)]
struct State {
    host: HostState,
    env: Option<EnvFile>,
    site: String,
    running: BTreeSet<String>,
    enabled: BTreeSet<String>,
    events: Vec<String>,
    unhealthy: BTreeSet<String>,
    failing_commands: BTreeSet<String>,
}

/// A server in memory. Every operation is recorded as an event.
#[derive(Debug)]
pub struct FakeHost {
    server: Server,
    layout: Layout,
    ports: [u16; 2],
    frontend_ports: Option<[u16; 2]>,
    state: Mutex<State>,
}

impl FakeHost {
    /// A provisioned, empty `x86_64` server with glibc 2.39.
    pub fn new(config: &HarborConfig, server: &str) -> FakeHost {
        let server = config
            .server(server)
            .expect("the server is in the configuration")
            .clone();
        let state = State {
            host: HostState {
                provisioned: true,
                arch: "x86_64".into(),
                glibc: Some("2.39".into()),
                ..HostState::default()
            },
            ..State::default()
        };
        FakeHost {
            server,
            layout: Layout::new(config),
            ports: config.deploy.ports,
            frontend_ports: config.frontend.as_ref().map(|f| f.ports),
            state: Mutex::new(state),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("fake host lock")
    }

    /// Starts with releases `ids` (oldest first), the last one live in
    /// `live` and the one before it in the other slot.
    pub fn with_releases(self, ids: &[&str], live: Slot) -> FakeHost {
        {
            let mut state = self.lock();
            state.host.releases = ids
                .iter()
                .map(|id| ReleaseInfo {
                    id: (*id).to_owned(),
                    commit: String::new(),
                    subject: format!("subject of {id}"),
                })
                .collect();
            if let Some(last) = ids.last() {
                state.host.current = Some((*last).to_owned());
                state.host.live = Some(live);
                set_slot(&mut state.host, live, last);
                state.running.insert(self.layout.web_unit(live));
                state.enabled.insert(self.layout.web_unit(live));
                if ids.len() > 1 {
                    set_slot(&mut state.host, live.other(), ids[ids.len() - 2]);
                }
            }
        }
        self
    }

    /// The server is not provisioned.
    pub fn unprovisioned(self) -> FakeHost {
        self.lock().host.provisioned = false;
        self
    }

    /// The server's architecture.
    pub fn with_arch(self, arch: &str) -> FakeHost {
        self.lock().host.arch = arch.to_owned();
        self
    }

    /// Release `id` never passes its health check here.
    pub fn unhealthy(self, id: &str) -> FakeHost {
        self.lock().unhealthy.insert(id.to_owned());
        self
    }

    /// The app command `command` (`migrate`) fails here.
    pub fn failing(self, command: &str) -> FakeHost {
        self.lock().failing_commands.insert(command.to_owned());
        self
    }

    /// Everything that happened, in order.
    pub fn events(&self) -> Vec<String> {
        self.lock().events.clone()
    }

    /// The server's state now.
    pub fn state(&self) -> HostState {
        self.lock().host.clone()
    }

    /// The units running now.
    pub fn running(&self) -> Vec<String> {
        self.lock().running.iter().cloned().collect()
    }

    /// The units enabled at boot.
    pub fn enabled(&self) -> Vec<String> {
        self.lock().enabled.iter().cloned().collect()
    }

    /// The environment file last written.
    pub fn env(&self) -> Option<EnvFile> {
        self.lock().env.clone()
    }

    /// The Caddy site last written.
    pub fn site(&self) -> String {
        self.lock().site.clone()
    }

    fn event(&self, state: &mut State, text: String) {
        state.events.push(text);
    }

    fn slot_of_port(&self, port: u16) -> Option<Slot> {
        Slot::BOTH.into_iter().find(|s| {
            self.ports[s.index()] == port
                || self.frontend_ports.is_some_and(|p| p[s.index()] == port)
        })
    }
}

fn set_slot(host: &mut HostState, slot: Slot, release: &str) {
    match slot {
        Slot::Blue => host.blue = Some(release.to_owned()),
        Slot::Green => host.green = Some(release.to_owned()),
    }
}

fn slot_release(host: &HostState, slot: Slot) -> Option<&String> {
    match slot {
        Slot::Blue => host.blue.as_ref(),
        Slot::Green => host.green.as_ref(),
    }
}

impl Host for FakeHost {
    fn server(&self) -> &Server {
        &self.server
    }

    fn inspect(&self) -> Result<HostState> {
        let mut state = self.lock();
        self.event(&mut state, "inspect".into());
        Ok(state.host.clone())
    }

    fn upload(&self, release: &str, _archive: &Path) -> Result<()> {
        let mut state = self.lock();
        self.event(&mut state, format!("upload {release}"));
        if !state.host.has_release(release) {
            state.host.releases.push(ReleaseInfo {
                id: release.to_owned(),
                commit: String::new(),
                subject: String::new(),
            });
            state.host.releases.sort_by(|a, b| a.id.cmp(&b.id));
        }
        Ok(())
    }

    fn write_env(&self, env: &EnvFile) -> Result<()> {
        let mut state = self.lock();
        self.event(&mut state, format!("env ({} settings)", env.len()));
        state.env = Some(env.clone());
        Ok(())
    }

    fn run_app(&self, step: &str, release: Option<&str>, args: &[String]) -> Result<Output> {
        let mut state = self.lock();
        let from = release
            .map(str::to_owned)
            .or_else(|| state.host.current.clone())
            .unwrap_or_default();
        let command = args.first().cloned().unwrap_or_default();
        self.event(&mut state, format!("run {} from {from}", args.join(" ")));
        if state.failing_commands.contains(&command) {
            return Err(HarborError::Remote {
                server: self.server.name.clone(),
                step: step.to_owned(),
                code: Some(1),
                output: format!("{command} failed"),
            });
        }
        let output = if command == "migrate" {
            "Migrated: deals/m20261026_091000_add_probability\nMigrated: deals/m20261026_091100_index_stage\n".to_owned()
        } else {
            format!("{command} done\n")
        };
        Ok(Output::ok(output))
    }

    fn start(&self, process: SlotProcess, slot: Slot, release: &str, env: &EnvFile) -> Result<()> {
        let mut state = self.lock();
        let unit = match process {
            SlotProcess::Web => {
                set_slot(&mut state.host, slot, release);
                self.layout.web_unit(slot)
            }
            SlotProcess::Frontend => self.layout.frontend_unit(slot),
        };
        let addr = env
            .get("APP_HTTP__ADDR")
            .or(env.get("PORT"))
            .unwrap_or_default()
            .to_owned();
        self.event(&mut state, format!("start {unit} ({release}, {addr})"));
        state.running.insert(unit);
        Ok(())
    }

    fn health(&self, port: u16, path: &str, _timeout_secs: u64) -> Result<Health> {
        let mut state = self.lock();
        self.event(&mut state, format!("health {port}{path}"));
        let slot = self.slot_of_port(port);
        let release = slot.and_then(|s| slot_release(&state.host, s)).cloned();
        let status = match release {
            Some(id) if state.unhealthy.contains(&id) => "503",
            Some(_) => "200",
            None => "000",
        };
        Ok(Health {
            status: status.to_owned(),
            millis: 400,
        })
    }

    fn logs(&self, unit: &str, lines: u32) -> Result<String> {
        let mut state = self.lock();
        self.event(&mut state, format!("logs {unit} {lines}"));
        Ok(format!("{unit}: database connection refused\n"))
    }

    fn systemctl(&self, _step: &str, verb: &str, units: &[String]) -> Result<()> {
        let mut state = self.lock();
        for unit in units {
            self.event(&mut state, format!("{verb} {unit}"));
            match verb {
                "stop" => {
                    state.running.remove(unit);
                }
                "start" | "restart" => {
                    state.running.insert(unit.clone());
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn switch(
        &self,
        slot: Slot,
        release: &str,
        site: &str,
        enable: &[String],
        disable: &[String],
    ) -> Result<()> {
        let mut state = self.lock();
        self.event(&mut state, format!("switch {slot} {release}"));
        state.host.live = Some(slot);
        state.host.current = Some(release.to_owned());
        state.site = site.to_owned();
        for unit in enable {
            state.enabled.insert(unit.clone());
        }
        for unit in disable {
            state.enabled.remove(unit);
        }
        Ok(())
    }

    fn set_current(&self, release: &str) -> Result<()> {
        let mut state = self.lock();
        self.event(&mut state, format!("current {release}"));
        state.host.current = Some(release.to_owned());
        Ok(())
    }

    fn prune(&self, keep: u32) -> Result<Vec<String>> {
        let mut state = self.lock();
        let protected: Vec<String> = [
            state.host.current.clone(),
            state.host.blue.clone(),
            state.host.green.clone(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut ids: Vec<String> = state.host.releases.iter().map(|r| r.id.clone()).collect();
        ids.sort();
        ids.reverse();
        let removed: Vec<String> = ids
            .into_iter()
            .skip(keep as usize)
            .filter(|id| !protected.contains(id))
            .collect();
        state.host.releases.retain(|r| !removed.contains(&r.id));
        self.event(&mut state, format!("prune ({} removed)", removed.len()));
        Ok(removed)
    }
}
