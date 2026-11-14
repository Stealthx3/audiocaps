// SPDX-License-Identifier: LGPL-2.1-or-later
//! Realtime scheduling available to this process, read passively.
//!
//! Three ways an audio thread gets realtime priority on Linux: `RLIMIT_RTPRIO` (set by PAM limits
//! or the `audio` group), rtkit (the system-bus broker PipeWire and PulseAudio use) and the
//! xdg-desktop-portal Realtime interface (the way in from Flatpak and Snap). Nothing is changed:
//! rtkit is checked by name ownership and activatability, never started, and no thread is made
//! realtime. D-Bus reads run on a helper thread with a short deadline so a stuck bus can't hold the
//! probe.

use std::sync::mpsc;
use std::time::Duration;

use audiocaps_model::{
    LayerKind, Provenance, Realtime, RealtimeMethod, Source, UnknownReason, Value,
};

const BUS_DEADLINE: Duration = Duration::from_millis(120);

fn prov(source: Source, at: u64) -> Provenance {
    Provenance {
        layer: LayerKind::Environment,
        source,
        read_at_ms: at,
    }
}

fn dbus_prov(service: &str, path: &str, interface: &str, property: &str, at: u64) -> Provenance {
    prov(
        Source::DbusProperty {
            service: service.into(),
            path: path.into(),
            interface: interface.into(),
            property: property.into(),
        },
        at,
    )
}

fn rlimit(now: u64) -> RealtimeMethod {
    use nix::sys::resource::{Resource, getrlimit};
    match getrlimit(Resource::RLIMIT_RTPRIO) {
        Ok((soft, _hard)) => {
            let max = if soft == nix::sys::resource::RLIM_INFINITY {
                99
            } else {
                u32::try_from(soft.min(99)).unwrap_or(99)
            };
            RealtimeMethod {
                method: "rlimit".into(),
                grants: Value::measured(
                    max > 0,
                    prov(Source::api("getrlimit", "RLIMIT_RTPRIO"), now),
                ),
                max_priority: (max > 0).then_some(max),
            }
        }
        Err(e) => RealtimeMethod {
            method: "rlimit".into(),
            grants: Value::unknown(UnknownReason::ReadError, Some(e.to_string())),
            max_priority: None,
        },
    }
}

fn unknown(method: &str, reason: UnknownReason, detail: String) -> RealtimeMethod {
    RealtimeMethod {
        method: method.into(),
        grants: Value::unknown(reason, Some(detail)),
        max_priority: None,
    }
}

fn denied(e: &zbus::Error) -> UnknownReason {
    if e.to_string().contains("AccessDenied") {
        UnknownReason::NotGranted
    } else {
        UnknownReason::ReadError
    }
}

const RTKIT: &str = "org.freedesktop.RealtimeKit1";
const RTKIT_PATH: &str = "/org/freedesktop/RealtimeKit1";
const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_RT: &str = "org.freedesktop.portal.Realtime";

async fn rtkit(now: u64) -> RealtimeMethod {
    let conn = match zbus::Connection::system().await {
        Ok(c) => c,
        Err(e) => {
            return unknown(
                "rtkit",
                UnknownReason::NotGranted,
                format!("no access to the system bus: {e}"),
            );
        }
    };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
        return unknown("rtkit", UnknownReason::ReadError, "bus proxy".into());
    };
    let Ok(name) = zbus::names::BusName::try_from(RTKIT) else {
        return unknown("rtkit", UnknownReason::ReadError, "bus name".into());
    };
    match dbus.name_has_owner(name).await {
        Ok(true) => {}
        Ok(false) => {
            // Activatable counts: the first MakeThreadRealtime call starts it. We don't.
            let activatable = dbus
                .list_activatable_names()
                .await
                .map(|v| v.iter().any(|n| n.as_str() == RTKIT));
            return match activatable {
                Ok(a) => RealtimeMethod {
                    method: "rtkit".into(),
                    grants: Value::measured(
                        a,
                        prov(
                            Source::api("org.freedesktop.DBus", "ListActivatableNames"),
                            now,
                        ),
                    ),
                    max_priority: None,
                },
                Err(e) => unknown("rtkit", denied(&e.into()), "rtkit not running".into()),
            };
        }
        Err(e) => return unknown("rtkit", denied(&e.into()), "NameHasOwner".into()),
    }
    let proxy = match zbus::Proxy::new(&conn, RTKIT, RTKIT_PATH, RTKIT).await {
        Ok(p) => p,
        Err(e) => return unknown("rtkit", denied(&e), e.to_string()),
    };
    match proxy.get_property::<i32>("MaxRealtimePriority").await {
        Ok(p) => RealtimeMethod {
            method: "rtkit".into(),
            grants: Value::measured(
                p > 0,
                dbus_prov(RTKIT, RTKIT_PATH, RTKIT, "MaxRealtimePriority", now),
            ),
            max_priority: u32::try_from(p).ok().filter(|p| *p > 0),
        },
        Err(e) => unknown("rtkit", denied(&e), e.to_string()),
    }
}

async fn portal(now: u64) -> RealtimeMethod {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            return unknown(
                "portal",
                UnknownReason::NotExposed,
                format!("no session bus: {e}"),
            );
        }
    };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
        return unknown("portal", UnknownReason::ReadError, "bus proxy".into());
    };
    let Ok(name) = zbus::names::BusName::try_from(PORTAL) else {
        return unknown("portal", UnknownReason::ReadError, "bus name".into());
    };
    match dbus.name_has_owner(name).await {
        Ok(true) => {}
        Ok(false) => {
            return RealtimeMethod {
                method: "portal".into(),
                grants: Value::measured(
                    false,
                    prov(Source::api("org.freedesktop.DBus", "NameHasOwner"), now),
                ),
                max_priority: None,
            };
        }
        Err(e) => return unknown("portal", denied(&e.into()), "NameHasOwner".into()),
    }
    let proxy = match zbus::Proxy::new(&conn, PORTAL, PORTAL_PATH, PORTAL_RT).await {
        Ok(p) => p,
        Err(e) => return unknown("portal", denied(&e), e.to_string()),
    };
    match proxy.get_property::<i32>("MaxRealtimePriority").await {
        Ok(p) => RealtimeMethod {
            method: "portal".into(),
            grants: Value::measured(
                p > 0,
                dbus_prov(PORTAL, PORTAL_PATH, PORTAL_RT, "MaxRealtimePriority", now),
            ),
            max_priority: u32::try_from(p).ok().filter(|p| *p > 0),
        },
        // The portal runs but has no Realtime interface (old portal or backend).
        Err(zbus::Error::FDO(e))
            if matches!(
                *e,
                zbus::fdo::Error::UnknownInterface(_) | zbus::fdo::Error::InvalidArgs(_)
            ) =>
        {
            RealtimeMethod {
                method: "portal".into(),
                grants: Value::measured(
                    false,
                    dbus_prov(PORTAL, PORTAL_PATH, PORTAL_RT, "MaxRealtimePriority", now),
                ),
                max_priority: None,
            }
        }
        Err(e) => unknown("portal", denied(&e), e.to_string()),
    }
}

/// Reads every method. Live system only.
pub(crate) fn read(now: u64) -> Realtime {
    let mut methods = vec![rlimit(now)];
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("audiocaps-realtime".into())
        .spawn(move || {
            let r = zruntime::block_on(async { (rtkit(now).await, portal(now).await) });
            let _ = tx.send(r);
        });
    match spawned
        .ok()
        .and_then(|_| rx.recv_timeout(BUS_DEADLINE).ok())
    {
        Some((r, p)) => methods.extend([r, p]),
        None => {
            for m in ["rtkit", "portal"] {
                methods.push(unknown(
                    m,
                    UnknownReason::Timeout,
                    "D-Bus didn't answer in time".into(),
                ));
            }
        }
    }
    Realtime {
        available: summarize(&methods),
        methods,
    }
}

fn summarize(methods: &[RealtimeMethod]) -> Option<bool> {
    if methods.iter().any(|m| m.grants.known() == Some(&true)) {
        Some(true)
    } else if methods.iter().all(|m| m.grants.known().is_some()) {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: Option<bool>) -> RealtimeMethod {
        RealtimeMethod {
            method: "x".into(),
            grants: match v {
                Some(b) => Value::measured(b, prov(Source::derived("test"), 0)),
                None => Value::unknown(UnknownReason::NotGranted, None),
            },
            max_priority: None,
        }
    }

    #[test]
    fn summary() {
        assert_eq!(
            summarize(&[m(Some(false)), m(None), m(Some(true))]),
            Some(true)
        );
        assert_eq!(summarize(&[m(Some(false)), m(None)]), None);
        assert_eq!(summarize(&[m(Some(false)), m(Some(false))]), Some(false));
    }
}
