// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
// RemotePairing request shapes follow pymobiledevice3/remote/native_tunnel.py.
// TODO: we should extend native support
use anyhow::{Context, Result, ensure};
use cpp::cpp;
use qttypes::QString;
cpp! {{
    #ifdef __APPLE__
    #include <xpc/xpc.h>
    #include <dispatch/dispatch.h>
    #include <QString>
    #include <memory>
    #include <mutex>
    #include <condition_variable>
    #include <chrono>
    #include <string>
    struct DdiNativeState {
        std::mutex mutex;
        std::condition_variable wake;
        xpc_object_t endpoint = nullptr;
        std::string udid;
        bool stopped = false;
        ~DdiNativeState() { if (endpoint) xpc_release(endpoint); }
    };
    struct DdiNativeSession {
        dispatch_queue_t queue = dispatch_queue_create("org.idescriptor.ddi.native", DISPATCH_QUEUE_SERIAL);
        xpc_connection_t browse = nullptr;
        xpc_connection_t device = nullptr;
        xpc_object_t assertion = nullptr;
        std::shared_ptr<DdiNativeState> state = std::make_shared<DdiNativeState>();
        static xpc_object_t message(const char *name, xpc_object_t body) {
            auto message = xpc_dictionary_create(nullptr, nullptr, 0);
            xpc_dictionary_set_string(message, "mangledTypeName", name);
            xpc_dictionary_set_value(message, "value", body);
            return message;
        }
        xpc_object_t request(const char *name, xpc_object_t body) {
            struct Reply {
                std::mutex mutex; std::condition_variable wake;
                xpc_object_t value = nullptr;
                ~Reply() { if (value) xpc_release(value); }
            };
            auto reply = std::make_shared<Reply>();
            auto msg = message(name, body);
            xpc_connection_send_message_with_reply(device, msg, queue, ^(xpc_object_t result) {
                std::lock_guard<std::mutex> lock(reply->mutex);
                reply->value = xpc_retain(result); reply->wake.notify_all();
            });
            xpc_release(msg);
            std::unique_lock<std::mutex> lock(reply->mutex);
            if (!reply->wake.wait_for(lock, std::chrono::seconds(10), [&] { return reply->value != nullptr; })) return nullptr;
            return xpc_retain(reply->value);
        }
        QString open(const QString &udid) {
            state->udid = udid.toStdString();
            browse = xpc_connection_create_mach_service("com.apple.CoreDevice.remotepairingd", queue, 0);
            auto shared = state;
            xpc_connection_set_event_handler(browse, ^(xpc_object_t event) {
                if (xpc_get_type(event) != XPC_TYPE_DICTIONARY) return;
                auto value = xpc_dictionary_get_value(event, "value");
                if (!value || xpc_get_type(value) != XPC_TYPE_DICTIONARY) return;
                auto found = xpc_dictionary_get_value(value, "deviceFound");
                if (!found || xpc_get_type(found) != XPC_TYPE_DICTIONARY) return;
                auto zero = xpc_dictionary_get_value(found, "_0");
                if (!zero || xpc_get_type(zero) != XPC_TYPE_DICTIONARY) return;
                auto info = xpc_dictionary_get_value(zero, "deviceInfo");
                if (!info || xpc_get_type(info) != XPC_TYPE_DICTIONARY) return;
                auto serial = xpc_dictionary_get_string(info, "udid");
                if (!serial || shared->udid != serial) return;
                auto endpoint = xpc_dictionary_get_value(info, "endpoint");
                if (!endpoint || xpc_get_type(endpoint) != XPC_TYPE_ENDPOINT) return;
                std::lock_guard<std::mutex> lock(shared->mutex);
                if (!shared->stopped && !shared->endpoint) {
                    shared->endpoint = xpc_retain(endpoint); shared->wake.notify_all();
                }
            });
            xpc_connection_resume(browse);
            auto body = xpc_dictionary_create(nullptr, nullptr, 0);
            xpc_dictionary_set_bool(body, "currentDevicesOnly", false);
            auto msg = message("RemotePairing.BrowseRequest", body);
            xpc_connection_send_message_with_reply(browse, msg, queue, ^(xpc_object_t) {});
            xpc_release(msg); xpc_release(body);
            {
                std::unique_lock<std::mutex> lock(state->mutex);
                if (!state->wake.wait_for(lock, std::chrono::seconds(10), [&] { return state->endpoint != nullptr; })) return {};
                device = xpc_connection_create_from_endpoint((xpc_endpoint_t)state->endpoint);
            }
            xpc_connection_set_target_queue(device, queue);
            xpc_connection_set_event_handler(device, ^(xpc_object_t) {});
            xpc_connection_resume(device);
            // Do not initiate pairing here: normal iDescriptor pairing already ran.
            body = xpc_dictionary_create(nullptr, nullptr, 0);
            xpc_dictionary_set_int64(body, "flags", 0);
            auto response = request("RemotePairing.CreateAssertionCommand", body);
            xpc_release(body);
            if (!response || xpc_get_type(response) != XPC_TYPE_DICTIONARY) {
                if (response) xpc_release(response); return {};
            }
            auto result = xpc_dictionary_get_value(response, "response");
            QString address;
            if (result && xpc_get_type(result) == XPC_TYPE_DICTIONARY) {
                auto id = xpc_dictionary_get_value(result, "assertionIdentifier");
                if (id) assertion = xpc_retain(id);
                auto info = xpc_dictionary_get_value(result, "info");
                if (info && xpc_get_type(info) == XPC_TYPE_DICTIONARY) {
                    auto ip = xpc_dictionary_get_string(info, "tunnelIPAddress");
                    if (ip) address = QString::fromUtf8(ip);
                }
            }
            xpc_release(response);
            return address;
        }
        ~DdiNativeSession() {
            { std::lock_guard<std::mutex> lock(state->mutex); state->stopped = true; }
            if (assertion && device) {
                auto body = xpc_dictionary_create(nullptr, nullptr, 0);
                xpc_dictionary_set_value(body, "assertionIdentifier", assertion);
                auto msg = message("RemotePairing.ReleaseAssertionRequest", body);
                xpc_connection_send_message_with_reply(device, msg, queue, ^(xpc_object_t) {});
                xpc_release(msg); xpc_release(body); xpc_release(assertion);
            }
            if (device) { xpc_connection_cancel(device); xpc_release(device); }
            if (browse) { xpc_connection_cancel(browse); xpc_release(browse); }
            dispatch_release(queue);
        }
    };
    #endif
}}
#[derive(Debug)]
pub struct NativeTunnel {
    handle: usize,
    pub address: String,
    pub rsd_port: u16,
}
impl NativeTunnel {
    pub async fn open(udid: &str) -> Result<Self> {
        let udid = udid.to_owned();
        let (handle, address) = tokio::task::spawn_blocking(move || {
            let udid = QString::from(udid);
            let mut handle = 0usize;
            let address = unsafe {
                cpp!([udid as "QString", mut handle as "size_t"] -> QString as "QString" {
                    #ifdef __APPLE__
                auto session = new DdiNativeSession();
                    auto address = session->open(udid);
                    if (address.isEmpty()) { delete session; return QString(); }
                    handle = reinterpret_cast<size_t>(session);
                    return address;
                    #else
                    (void)udid; (void)handle;
                    return QString();
                    #endif
                })
            };
            (handle, address.to_string())
        })
        .await?;
        ensure!(
            handle != 0 && !address.is_empty(),
            "Apple's native tunnel is unavailable for this paired device"
        );
        let mut tunnel = Self {
            handle,
            address,
            rsd_port: 0,
        };
        let mut command = tokio::process::Command::new("/usr/bin/nettop");
        command
            .args(["-n", "-x", "-L", "1", "-m", "tcp", "-J", "interface,state"])
            .kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(15), command.output())
            .await
            .context("Native tunnel port discovery timed out")??;
        ensure!(
            output.status.success(),
            "Could not inspect Apple's native tunnel connections"
        );
        tunnel.rsd_port = find_port(&String::from_utf8_lossy(&output.stdout), &tunnel.address)
            .context("Apple's tunnel has no established RSD connection")?;
        Ok(tunnel)
    }
}
fn find_port(text: &str, address: &str) -> Option<u16> {
    let target: std::net::Ipv6Addr = address.parse().ok()?;
    let mut remoted = false;
    for line in text.lines() {
        let fields = line.split(',').collect::<Vec<_>>();
        let first = fields.first()?.trim();
        if !first.contains("<->") {
            remoted = first.starts_with("remoted.");
            continue;
        }
        if !remoted || fields.get(2).copied() != Some("Established") {
            continue;
        }
        let foreign = first.split("<->").nth(1)?.trim();
        let Some((host, port)) = foreign.rsplit_once('.') else {
            continue;
        };
        let Ok(host) = host
            .split('%')
            .next()
            .unwrap_or(host)
            .parse::<std::net::Ipv6Addr>()
        else {
            continue;
        };
        if host == target {
            return port.parse().ok();
        }
    }
    None
}
impl Drop for NativeTunnel {
    fn drop(&mut self) {
        let handle = self.handle;
        // The session owns its assertion and connections through the last upload.
        unsafe {
            cpp!([handle as "size_t"] {
                       #ifdef __APPLE__
                       delete reinterpret_cast<DdiNativeSession*>(handle);
                       #else
                       (void)handle;
                       #endif
            });
        }
    }
}
