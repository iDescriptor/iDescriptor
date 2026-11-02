// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::device_ctx::DeviceServices;
use anyhow::{Context, Result, ensure};
use idevice::{
    IdeviceService, ReadWrite, provider::RsdProvider, rsd::RsdHandshake,
    services::core_device_proxy::CoreDeviceProxy,
};
use std::time::Duration;

#[derive(Debug)]
pub enum Transport {
    Software(idevice::tcp::handle::AdapterHandle),
    #[cfg(target_os = "macos")]
    Native(super::native::NativeTunnel),
}
impl RsdProvider for Transport {
    async fn connect_to_service_port(
        &mut self,
        port: u16,
    ) -> Result<Box<dyn ReadWrite>, idevice::IdeviceError> {
        match self {
            Self::Software(adapter) => Ok(Box::new(adapter.connect(port).await?)),
            #[cfg(target_os = "macos")]
            Self::Native(tunnel) => Ok(Box::new(
                tokio::net::TcpStream::connect((tunnel.address.as_str(), port)).await?,
            )),
        }
    }
}
pub struct Connection {
    pub transport: Transport,
    pub handshake: RsdHandshake,
}
impl Connection {
    pub async fn open(device: &DeviceServices, udid: &str) -> Result<Self> {
        let primary = tokio::time::timeout(Duration::from_secs(25), async {
            let proxy = {
                let provider = device.provider.lock().await;
                CoreDeviceProxy::connect(provider.as_ref()).await?
            };
            let port = proxy.tunnel_info().server_rsd_port;
            let mut transport =
                Transport::Software(proxy.create_software_tunnel()?.to_async_handle());
            let stream = transport.connect_to_service_port(port).await?;
            let handshake = RsdHandshake::new(stream).await?;
            Ok::<_, anyhow::Error>(Self {
                transport,
                handshake,
            })
        })
        .await
        .context("Developer connection timed out")
        .and_then(|result| result);
        let connection = match primary {
            Ok(connection) => connection,
            Err(error) => {
                // FIXME: Native fallback only covers tunnel creation.
                #[cfg(target_os = "macos")]
                {
                    log::warn!(
                        "Developer tunnel unavailable; trying Apple's native tunnel: {error:#}"
                    );
                    let tunnel = super::native::NativeTunnel::open(udid).await
                        .with_context(|| format!("Developer tunnel failed ({error:#}); native tunnel fallback also failed"))?;
                    let port = tunnel.rsd_port;
                    let mut transport = Transport::Native(tunnel);
                    let stream = transport.connect_to_service_port(port).await?;
                    let handshake =
                        tokio::time::timeout(Duration::from_secs(20), RsdHandshake::new(stream))
                            .await??;
                    Self {
                        transport,
                        handshake,
                    }
                }
                #[cfg(not(target_os = "macos"))]
                return Err(error.context("Could not establish the developer tunnel"));
            }
        };
        if let Some(actual) = connection
            .handshake
            .properties
            .get("UniqueDeviceID")
            .and_then(plist::Value::as_string)
        {
            ensure!(
                actual == udid,
                "Developer tunnel connected to a different device"
            );
        }
        Ok(connection)
    }
}
