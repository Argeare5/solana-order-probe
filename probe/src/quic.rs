//! Direct QUIC client to a leader's TPU. The client is configured as Agave 4.3's own `solana-tpu-client-next`
//! configures it (ALPN `solana-tpu`, 10 s idle timeout, 1 s keep-alive, no send fairness, a large initial
//! congestion window), so a probe takes the same path as a transaction a validator forwards. The identity is a fresh,
//! unstaked keypair: the leader gives the connection the share of an unstaked peer (stake-weighted QoS).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use quinn::congestion::CubicConfig;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ClientConfig, Connection, Endpoint, IdleTimeout, TransportConfig};
use solana_tls_utils::{QuicClientCertificate, socket_addr_to_quic_server_name, tls_client_config_builder};

/// ALPN of the TPU (`solana_streamer::nonblocking::quic::ALPN_TPU_PROTOCOL_ID`).
const ALPN_TPU: &[u8] = b"solana-tpu";
const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const KEEP_ALIVE: Duration = Duration::from_secs(1);
/// Size of a packet on the TPU (`solana_packet::PACKET_DATA_SIZE`).
const PACKET_DATA_SIZE: u64 = 1232;

pub struct TpuClient {
    endpoint: Endpoint,
}

impl TpuClient {
    pub fn new() -> Result<Self> {
        let identity = tls_keypair::Keypair::new();
        let certificate = QuicClientCertificate::new(Some(&identity));
        let mut crypto = tls_client_config_builder()
            .with_client_auth_cert(vec![certificate.certificate.clone()], certificate.key.clone_key())
            .context("setting the QUIC client certificate")?;
        crypto.enable_early_data = true;
        crypto.alpn_protocols = vec![ALPN_TPU.to_vec()];

        let mut transport = TransportConfig::default();
        transport.max_idle_timeout(Some(IdleTimeout::try_from(IDLE_TIMEOUT)?));
        transport.keep_alive_interval(Some(KEEP_ALIVE));
        transport.datagram_receive_buffer_size(None);
        transport.send_fairness(false);
        let mut cubic = CubicConfig::default();
        cubic.initial_window(128 * PACKET_DATA_SIZE);
        transport.congestion_controller_factory(Arc::new(cubic));

        let mut config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));
        config.transport_config(Arc::new(transport));
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?).context("binding the QUIC endpoint")?;
        endpoint.set_default_client_config(config);
        Ok(Self { endpoint })
    }

    /// Opens a connection to `peer` and returns it with the handshake time.
    pub async fn connect(&self, peer: SocketAddr) -> Result<(Connection, Duration)> {
        let started = Instant::now();
        let connecting = self.endpoint.connect(peer, &socket_addr_to_quic_server_name(peer))?;
        let connection = tokio::time::timeout(Duration::from_secs(3), connecting)
            .await
            .context("QUIC handshake timed out")?
            .context("QUIC handshake failed")?;
        Ok((connection, started.elapsed()))
    }
}

/// Sends one wire transaction on its own unidirectional stream, as the TPU expects. The finished stream is returned
/// so the caller can wait for the TPU's acknowledgement (`SendStream::stopped`) after the whole burst is out.
pub async fn send(connection: &Connection, wire: &[u8]) -> Result<quinn::SendStream> {
    let mut stream = connection.open_uni().await?;
    stream.write_all(wire).await?;
    stream.finish()?;
    Ok(stream)
}
