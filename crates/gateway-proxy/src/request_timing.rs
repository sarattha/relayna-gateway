//! Bounded observations that export only timing metadata and never modify chunks.
use gateway_core::traffic::TrafficRequest;
use pingora_core::protocols::Digest;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

const TOKEN_PROBE_LIMIT: usize = 65_536;

#[derive(Debug, Default)]
pub(crate) struct RequestTimingProbe {
    pub tcp_started: Arc<Mutex<Option<SystemTime>>>,
    token_prefix: Vec<u8>,
    token_probe_finished: bool,
    token_bytes_seen: usize,
}

pub(crate) fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

impl RequestTimingProbe {
    pub fn connected(&self, traffic: &mut TrafficRequest, reused: bool, digest: Option<&Digest>) {
        let Some(timing) = traffic.upstream_timings.last_mut() else {
            return;
        };
        timing.connection_reused = Some(reused);
        if reused {
            return;
        }
        let tcp_end = digest
            .and_then(|d| d.timing_digest.first())
            .and_then(Option::as_ref)
            .map(|d| d.established_ts);
        let tcp_start = self.tcp_started.lock().ok().and_then(|v| *v);
        timing.tcp_connect_us = tcp_end
            .zip(tcp_start)
            .and_then(|(end, start)| end.duration_since(start).ok())
            .map(micros);
        if timing.tls {
            let tls_end = digest
                .and_then(|d| d.timing_digest.get(1))
                .and_then(Option::as_ref)
                .map(|d| d.established_ts);
            timing.tls_handshake_us = tls_end
                .zip(tcp_end)
                .and_then(|(end, start)| end.duration_since(start).ok())
                .map(micros);
        }
    }

    pub fn body(&mut self, traffic: &mut TrafficRequest, body: &[u8], elapsed_ms: i64) {
        let Some(timing) = traffic.upstream_timings.last_mut() else {
            return;
        };
        if body.is_empty() {
            return;
        }
        let since_attempt = elapsed_ms.saturating_sub(timing.started_elapsed_ms);
        timing.first_body_byte_ms.get_or_insert(since_attempt);
        if !traffic.streaming
            || self.token_probe_finished
            || timing.upstream_status.is_some_and(|s| s >= 400)
        {
            return;
        }
        let remaining = TOKEN_PROBE_LIMIT.saturating_sub(self.token_bytes_seen);
        self.token_bytes_seen += body.len().min(remaining);
        self.token_prefix
            .extend_from_slice(&body[..body.len().min(remaining)]);
        // Only complete SSE events are parsed, once. Retain at most the incomplete
        // event and stop after 64 KiB of inspected bytes, even for endless keepalives.
        let mut consumed = 0;
        while let Some((end, separator)) = self.token_prefix[consumed..]
            .windows(2)
            .position(|w| w == b"\n\n")
            .map(|i| (i, 2))
            .into_iter()
            .chain(
                self.token_prefix[consumed..]
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|i| (i, 4)),
            )
            .min_by_key(|(i, _)| *i)
        {
            let event = String::from_utf8_lossy(&self.token_prefix[consumed..consumed + end]);
            let data = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            let found = serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .is_some_and(|v| has_content_delta(&v));
            consumed += end + separator;
            if found {
                timing.first_token_ms = Some(since_attempt);
                self.token_probe_finished = true;
                break;
            }
        }
        if self.token_bytes_seen == TOKEN_PROBE_LIMIT {
            self.token_probe_finished = true;
        }

        if self.token_probe_finished {
            self.token_prefix.clear();
        } else if consumed > 0 {
            self.token_prefix.drain(..consumed);
        }
    }
}

fn has_content_delta(value: &serde_json::Value) -> bool {
    let nonempty = |v: Option<&serde_json::Value>| {
        v.and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.is_empty())
    };
    if value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|c| {
                nonempty(c.pointer("/delta/content"))
                    || nonempty(c.get("text"))
                    || c.pointer("/delta/tool_calls")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|calls| {
                            calls
                                .iter()
                                .any(|call| nonempty(call.pointer("/function/arguments")))
                        })
            })
        })
    {
        return true;
    }
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("response.output_text.delta" | "response.function_call_arguments.delta") => {
            nonempty(value.get("delta"))
        }
        Some("content_block_delta") => {
            nonempty(value.pointer("/delta/text")) || nonempty(value.pointer("/delta/partial_json"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::traffic::UpstreamTiming;
    use pingora_core::protocols::TimingDigest;

    #[test]
    fn resolved_peer_preserves_pingora_address_selection() {
        use pingora_core::upstreams::peer::HttpPeer;
        let addresses: [std::net::SocketAddr; 2] = [
            "[::1]:443".parse().unwrap(),
            "127.0.0.1:443".parse().unwrap(),
        ];
        let native = HttpPeer::new(addresses.as_slice(), true, "localhost".into());
        let resolved = HttpPeer::new(addresses[0], true, "localhost".into());
        assert_eq!(native._address.as_inet(), Some(&addresses[0]));
        assert_eq!(resolved._address.as_inet(), native._address.as_inet());
        assert_eq!(resolved.sni, native.sni);
        assert!(resolved.options.verify_cert && resolved.options.verify_hostname);
    }

    #[tokio::test]
    async fn real_tls_digest_measures_handshake_and_rejects_wrong_hostname() {
        use openssl::{
            asn1::Asn1Time,
            hash::MessageDigest,
            pkey::PKey,
            rsa::Rsa,
            ssl::{SslAcceptor, SslMethod},
            x509::{extension::SubjectAlternativeName, X509NameBuilder, X509},
        };
        use pingora_core::{
            connectors::{http::Connector, ConnectorOptions},
            upstreams::peer::HttpPeer,
        };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let mut name = X509NameBuilder::new().unwrap();
        name.append_entry_by_text("CN", "localhost").unwrap();
        let name = name.build();
        let mut cert = X509::builder().unwrap();
        cert.set_version(2).unwrap();
        cert.set_subject_name(&name).unwrap();
        cert.set_issuer_name(&name).unwrap();
        cert.set_pubkey(&key).unwrap();
        cert.set_not_before(&Asn1Time::days_from_now(0).unwrap())
            .unwrap();
        cert.set_not_after(&Asn1Time::days_from_now(1).unwrap())
            .unwrap();
        let san = SubjectAlternativeName::new()
            .dns("localhost")
            .build(&cert.x509v3_context(None, None))
            .unwrap();
        cert.append_extension(san).unwrap();
        cert.sign(&key, MessageDigest::sha256()).unwrap();
        let cert = cert.build();
        let ca_path =
            std::env::temp_dir().join(format!("gateway-timing-{}.pem", uuid::Uuid::new_v4()));
        std::fs::write(&ca_path, cert.to_pem().unwrap()).unwrap();
        let connector = Connector::new(Some(ConnectorOptions {
            ca_file: Some(ca_path.to_str().unwrap().into()),
            ..ConnectorOptions::new(8)
        }));
        std::fs::remove_file(ca_path).unwrap();
        let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
        acceptor.set_certificate(&cert).unwrap();
        acceptor.set_private_key(&key).unwrap();
        let acceptor = acceptor.build();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut accepted = 0;
            while accepted < 2 && std::time::Instant::now() < deadline {
                if let Ok((stream, _)) = listener.accept() {
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let _ = acceptor.accept(stream);
                    accepted += 1;
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        let probe = RequestTimingProbe::default();
        let start = probe.tcp_started.clone();
        let mut peer = HttpPeer::new(address, true, "localhost".into());
        peer.options.connection_timeout = Some(Duration::from_secs(3));
        peer.options.total_connection_timeout = Some(Duration::from_secs(3));
        peer.options.upstream_tcp_sock_tweak_hook = Some(Arc::new(move |_| {
            *start.lock().unwrap() = Some(SystemTime::now());
            Ok(())
        }));
        let (session, reused) = connector.get_http_session(&peer).await.unwrap();
        let mut traffic = TrafficRequest {
            upstream_timings: vec![UpstreamTiming {
                tls: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        probe.connected(&mut traffic, reused, session.digest());
        assert!(!reused);
        assert!(traffic.upstream_timings[0].tcp_connect_us.is_some());
        assert!(traffic.upstream_timings[0]
            .tls_handshake_us
            .is_some_and(|us| us > 0));
        drop(session);
        peer.sni = "wrong-host.invalid".into();
        assert!(
            connector.get_http_session(&peer).await.is_err(),
            "hostname verification must stay enabled"
        );
        server.join().unwrap();
    }

    #[test]
    fn connection_timings_distinguish_fresh_reused_plain_and_missing() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let probe = RequestTimingProbe::default();
        *probe.tcp_started.lock().unwrap() = Some(start);
        let digest = Digest {
            timing_digest: vec![
                Some(TimingDigest {
                    established_ts: start + Duration::from_micros(850),
                }),
                Some(TimingDigest {
                    established_ts: start + Duration::from_micros(3350),
                }),
            ],
            ..Default::default()
        };
        let mut traffic = TrafficRequest {
            upstream_timings: vec![UpstreamTiming {
                tls: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        probe.connected(&mut traffic, false, Some(&digest));
        assert_eq!(traffic.upstream_timings[0].tcp_connect_us, Some(850));
        assert_eq!(traffic.upstream_timings[0].tls_handshake_us, Some(2500));
        for (reused, tls, available) in [
            (true, true, true),
            (false, false, true),
            (false, true, false),
        ] {
            traffic.upstream_timings = vec![UpstreamTiming {
                tls,
                ..Default::default()
            }];
            probe.connected(&mut traffic, reused, available.then_some(&digest));
            assert_eq!(traffic.upstream_timings[0].tls_handshake_us, None);
            assert_eq!(
                traffic.upstream_timings[0].tcp_connect_us,
                if !reused && available {
                    Some(850)
                } else {
                    None
                }
            );
        }
        *probe.tcp_started.lock().unwrap() = Some(start + Duration::from_secs(5));
        traffic.upstream_timings = vec![UpstreamTiming::default()];
        probe.connected(&mut traffic, false, Some(&digest));
        assert_eq!(
            traffic.upstream_timings[0].tcp_connect_us, None,
            "clock discontinuities must not become zero"
        );
    }

    #[test]
    fn first_token_waits_for_content_and_handles_every_chunk_boundary() {
        let prefix =
            b": keepalive\n\ndata: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n";
        let content = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\r\n\r\n";
        for split in 0..content.len() {
            let mut traffic = TrafficRequest {
                streaming: true,
                upstream_timings: vec![UpstreamTiming {
                    started_elapsed_ms: 10,
                    ..Default::default()
                }],
                ..Default::default()
            };
            let mut probe = RequestTimingProbe::default();
            probe.body(&mut traffic, prefix, 30);
            assert_eq!(traffic.upstream_timings[0].first_token_ms, None);
            probe.body(&mut traffic, &content[..split], 40);
            probe.body(&mut traffic, &content[split..], 50);
            assert_eq!(traffic.upstream_timings[0].first_body_byte_ms, Some(20));
            assert_eq!(traffic.upstream_timings[0].first_token_ms, Some(40));
            probe.body(&mut traffic, content, 90);
            assert_eq!(traffic.upstream_timings[0].first_token_ms, Some(40));
        }
        assert!(has_content_delta(
            &serde_json::json!({"type":"response.output_text.delta","delta":"hi"})
        ));
        assert!(has_content_delta(
            &serde_json::json!({"type":"content_block_delta","delta":{"text":"hi"}})
        ));
    }

    #[test]
    fn token_probe_is_bounded_and_does_not_change_chunks() {
        let mut traffic = TrafficRequest {
            streaming: true,
            upstream_timings: vec![UpstreamTiming::default()],
            ..Default::default()
        };
        let mut probe = RequestTimingProbe::default();
        let chunk = vec![b'x'; TOKEN_PROBE_LIMIT + 100];
        probe.body(&mut traffic, &chunk, 1);
        assert!(probe.token_prefix.is_empty());
        assert!(probe.token_probe_finished);
        assert_eq!(traffic.upstream_timings[0].first_token_ms, None);
        assert_eq!(chunk.len(), TOKEN_PROBE_LIMIT + 100);
    }
}
