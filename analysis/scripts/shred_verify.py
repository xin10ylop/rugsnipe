#!/usr/bin/env python3
"""Offline integrity check for a Solana shred capture (pcap from tcpdump, or the
proxy's borsh bins/serialized_shreds.bin). For every Merkle shred it recomputes the
Merkle root from the shred's own leaf + proof (same algorithm as agave
ledger/src/shred/merkle.rs), groups shreds by (slot, fec_set_index) and reports:
  - FEC sets whose shreds disagree on root or leader signature (=> recovery fails
    with InvalidMerkleRoot no matter what code you run),
  - with --rpc, ed25519 verification of the root against the slot leader,
  - completeness: share of FEC sets with all data shreds / >= num_data total shreds.
Usage: python3 -I shred_verify.py capture.pcap [--port 20000] [--rpc https://api.mainnet-beta.solana.com]
"""
import sys, struct, hashlib, json, argparse, collections, urllib.request

LEAF = b"\x00SOLANA_MERKLE_SHREDS_LEAF"
NODE = b"\x01SOLANA_MERKLE_SHREDS_NODE"
DATA_LEN, CODE_LEN = 1203, 1228

def h(*parts):
    m = hashlib.sha256()
    for p in parts: m.update(p)
    return m.digest()

def parse(buf):
    if len(buf) < 89: return None
    v = buf[64]; hi, ps = v & 0xF0, v & 0x0F
    kinds = {0x40: ("code", False, False), 0x60: ("code", True, False), 0x70: ("code", True, True),
             0x80: ("data", False, False), 0x90: ("data", True, False), 0xb0: ("data", True, True)}
    if hi not in kinds: return None  # legacy / unknown
    kind, chained, resigned = kinds[hi]
    size = DATA_LEN if kind == "data" else CODE_LEN
    if len(buf) < size: return {"bad": "short", "len": len(buf)}
    buf = buf[:size]
    slot, index, version, fec = struct.unpack_from("<QIHI", buf, 65)
    proof_off = size - 20 * ps - (64 if resigned else 0)
    if kind == "data":
        shard = index - fec
        nd = nc = None
    else:
        nd, nc, pos = struct.unpack_from("<HHH", buf, 83)
        shard = nd + pos
    node = h(LEAF, buf[64:proof_off])
    idx = shard
    for i in range(ps):
        e = buf[proof_off + 20 * i: proof_off + 20 * i + 20]
        node = h(NODE, node[:20], e) if idx % 2 == 0 else h(NODE, e, node[:20])
        idx >>= 1
    return {"kind": kind, "slot": slot, "index": index, "fec": fec, "version": version,
            "variant": v, "shard": shard, "nd": nd, "nc": nc, "sig": bytes(buf[:64]),
            "root": node if idx == 0 else None, "chained": chained, "resigned": resigned,
            "body": bytes(buf[:size - (64 if resigned else 0)])}

def read_pcap(path, port):
    with open(path, "rb") as f: d = f.read()
    magic = d[:4]
    if magic in (b"\xd4\xc3\xb2\xa1", b"\x4d\x3c\xb2\xa1"): e = "<"
    elif magic in (b"\xa1\xb2\xc3\xd4", b"\xa1\xb2\x3c\x4d"): e = ">"
    else: sys.exit("not a classic pcap (use tcpdump -w, not pcapng)")
    linktype = struct.unpack_from(e + "I", d, 20)[0]
    off = 24
    while off + 16 <= len(d):
        _, _, incl, _ = struct.unpack_from(e + "IIII", d, off); off += 16
        pkt = d[off:off + incl]; off += incl
        if linktype == 1: l3, et = 14, struct.unpack_from(">H", pkt, 12)[0]
        elif linktype == 113: l3, et = 16, struct.unpack_from(">H", pkt, 14)[0]
        elif linktype == 276: l3, et = 20, struct.unpack_from(">H", pkt, 0)[0]
        elif linktype in (101, 228): l3, et = 0, 0x0800
        else: sys.exit(f"unsupported linktype {linktype}")
        if et == 0x8100: et = struct.unpack_from(">H", pkt, l3 + 2)[0]; l3 += 4
        if et != 0x0800 or len(pkt) < l3 + 28 or pkt[l3 + 9] != 17: continue
        ihl = (pkt[l3] & 0x0F) * 4
        sport, dport, ulen = struct.unpack_from(">HHH", pkt, l3 + ihl)
        if port and dport != port: continue
        yield pkt[l3 + ihl + 8: l3 + ihl + ulen]

def read_borsh(path):
    with open(path, "rb") as f: d = f.read()
    n = struct.unpack_from("<I", d, 0)[0]; off = 4
    for _ in range(n):
        l = struct.unpack_from("<I", d, off)[0]; off += 4
        yield d[off:off + l]; off += l

B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
def b58d(s):
    n = 0
    for c in s: n = n * 58 + B58.index(c)
    b = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return b"\x00" * (len(s) - len(s.lstrip("1"))) + b

def leaders(rpc, start, count):
    out = {}
    while count > 0:
        k = min(count, 5000)
        req = urllib.request.Request(rpc, json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getSlotLeaders",
                                     "params": [start, k]}).encode(), {"Content-Type": "application/json"})
        res = json.load(urllib.request.urlopen(req, timeout=30))["result"]
        for i, pk in enumerate(res): out[start + i] = pk
        start += k; count -= k
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("file"); ap.add_argument("--port", type=int, default=20000)
    ap.add_argument("--rpc"); ap.add_argument("--show", type=int, default=5)
    a = ap.parse_args()
    src = read_borsh(a.file) if a.file.endswith(".bin") else read_pcap(a.file, a.port)
    fec = collections.defaultdict(list); stats = collections.Counter(); seen = {}
    for p in src:
        s = parse(p)
        if s is None: stats["non_merkle_or_unparsed"] += 1; continue
        if "bad" in s: stats["short_payload"] += 1; continue
        stats[f"{s['kind']}_variant_{s['variant'] & 0xF0:#x}"] += 1
        key = (s["slot"], s["index"], s["kind"])
        if key in seen:
            stats["dup_identical" if seen[key] == s["body"] else "dup_CONFLICTING_bytes"] += 1
            continue
        seen[key] = s["body"]
        if s["root"] is None: stats["bad_proof_index"] += 1
        fec[(s["slot"], s["fec"])].append(s)
    lead = {}
    if a.rpc and fec:
        slots = sorted({k[0] for k in fec}); lead = leaders(a.rpc, slots[0], slots[-1] - slots[0] + 1)
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    bad_sets = []; comp = collections.Counter()
    for (slot, fi), ss in sorted(fec.items()):
        roots = {s["root"] for s in ss}; sigs = {s["sig"] for s in ss}
        code = [s for s in ss if s["kind"] == "code"]; data = [s for s in ss if s["kind"] == "data"]
        droots = {s["root"] for s in data}; croots = {s["root"] for s in code}
        nd = code[0]["nd"] if code else None
        ok = len(roots) == 1 and len(sigs) == 1 and None not in roots
        if lead.get(slot):
            pk = Ed25519PublicKey.from_public_bytes(b58d(lead[slot]))
            nver = 0
            for s in ss:
                try: pk.verify(s["sig"], s["root"]); s["v"] = True; nver += 1
                except Exception: s["v"] = False
            comp["shreds_sig_ok"] += nver; comp["shreds_sig_fail"] += len(ss) - nver
            comp["data_sig_fail"] += sum(1 for s in data if not s["v"])
            comp["code_sig_fail"] += sum(1 for s in code if not s["v"])
        comp["fec_sets"] += 1
        comp["fec_consistent" if ok else "fec_INCONSISTENT"] += 1
        if not ok: bad_sets.append((slot, fi, len(data), len(code), len(droots), len(croots), len(sigs),
                                    bool(droots & croots)))
        if nd:
            comp["fec_with_code_hdr"] += 1
            if len(data) >= nd: comp["fec_all_data"] += 1
            if len(data) + len(code) >= nd: comp["fec_recoverable_by_count"] += 1
    print("shred counts:", dict(stats)); print("fec summary:", dict(comp))
    for b in bad_sets[:a.show]:
        print("INCONSISTENT slot %d fec %d: data=%d code=%d distinct_data_roots=%d distinct_code_roots=%d "
              "distinct_sigs=%d data_and_code_share_a_root=%s" % b)

if __name__ == "__main__":
    main()
