#!/usr/bin/env python3
"""Probe how a Solana TPU endpoint (validator, BAM node, Harmonic Remote TPU) admits QUIC clients.

Run it from the sending box (Frankfurt/Amsterdam), NOT from a CI container: it needs raw UDP egress.
It sends NO transactions. It only opens QUIC connections with ALPN "solana-tpu" and watches how
the server closes them. Agave/jito-relayer close codes (streamer/src/nonblocking/quic.rs):
  1 "dropped"   -> evicted from the connection table (prune_oldest / prune_random)
  2 "disallowed"-> per-IP connection-rate limit, global rate limit, or unstaked disallowed
  3 "exceed_max_stream_count", 4 "too_many" (per-peer connection cap), 5 "invalid_stream"

Modes
  hold  <secs> host:port ...   keep idle connections open, ping every 0.5 s, report close code and time
  burst <n> host:port          open n connections in a row (each held 0.5 s). Agave >=3.x allows
                               8 new connections per IP per minute; jito-relayer (streamer 2.2) allows 64.
                               The n-th connection that gets code 2 fingerprints the server's code lineage.
  --identity keypair.json      present this Solana identity in the client cert (to test a staked key)

pip install aioquic cryptography
"""
import argparse, asyncio, contextlib, datetime, json, socket, ssl, sys, time
from aioquic.asyncio.protocol import QuicConnectionProtocol
from aioquic.quic.configuration import QuicConfiguration
from aioquic.quic.connection import QuicConnection
from aioquic.quic.events import ConnectionTerminated
from cryptography import x509
from cryptography.hazmat.primitives.asymmetric import ed25519
from cryptography.x509.oid import NameOID


def make_cert(identity_path=None):
    if identity_path:
        raw = bytes(json.load(open(identity_path)))
        key = ed25519.Ed25519PrivateKey.from_private_bytes(raw[:32])
    else:
        key = ed25519.Ed25519PrivateKey.generate()
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Solana node")])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
            .public_key(key.public_key()).serial_number(1)
            .not_valid_before(now - datetime.timedelta(days=1))
            .not_valid_after(now + datetime.timedelta(days=365)).sign(key, None))
    return cert, key


class Proto(QuicConnectionProtocol):
    def __init__(self, *a, **kw):
        super().__init__(*a, **kw)
        self.term = None
        self.t_term = None

    def quic_event_received(self, ev):
        if isinstance(ev, ConnectionTerminated):
            self.term = (ev.error_code, ev.reason_phrase)
            self.t_term = time.monotonic()


@contextlib.asynccontextmanager
async def connect(host, port, cfg):
    loop = asyncio.get_running_loop()
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("0.0.0.0", 0))
    transport, proto = await loop.create_datagram_endpoint(lambda: Proto(QuicConnection(configuration=cfg)), sock=sock)
    try:
        proto.connect((host, port), transmit=True)
        await asyncio.wait_for(proto.wait_connected(), 5)
        yield proto
    finally:
        proto.close()
        with contextlib.suppress(Exception):
            await asyncio.wait_for(proto.wait_closed(), 2)
        transport.close()


async def probe(target, hold, identity):
    host, port = target.rsplit(":", 1)
    cfg = QuicConfiguration(is_client=True, alpn_protocols=["solana-tpu"], verify_mode=ssl.CERT_NONE,
                            idle_timeout=60.0, server_name="localhost")
    cfg.certificate, cfg.private_key = make_cert(identity)
    t0 = time.monotonic()
    res = {"target": target, "utc": datetime.datetime.utcnow().isoformat(timespec="seconds")}
    try:
        async with connect(host, int(port), cfg) as c:
            res["handshake_ms"] = round((time.monotonic() - t0) * 1000, 2)
            t1 = time.monotonic()
            await asyncio.wait_for(c.ping(), 5)
            res["ping_ms"] = round((time.monotonic() - t1) * 1000, 2)
            end = time.monotonic() + hold
            while time.monotonic() < end and c.term is None:
                await asyncio.sleep(0.5)
                with contextlib.suppress(Exception):
                    await asyncio.wait_for(c.ping(), 3)
            if c.term:
                res["closed_after_s"] = round(c.t_term - t0, 2)
                res["close_code"], res["close_reason"] = c.term
            else:
                res["alive_after_s"] = hold
    except Exception as e:  # handshake refused / timed out
        res["error"] = repr(e)[:160]
        res["t_s"] = round(time.monotonic() - t0, 2)
    print(json.dumps(res), flush=True)
    return res


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["hold", "burst"])
    ap.add_argument("n", type=float, help="hold seconds, or burst count")
    ap.add_argument("targets", nargs="+")
    ap.add_argument("--identity")
    a = ap.parse_args()
    if a.mode == "hold":
        await asyncio.gather(*[probe(t, a.n, a.identity) for t in a.targets])
    else:
        for i in range(int(a.n)):
            r = await probe(a.targets[0], 0.5, a.identity)
            if r.get("close_code") == 2 or "error" in r:
                print(json.dumps({"burst_refused_at": i + 1}), flush=True)


if __name__ == "__main__":
    asyncio.run(main())
