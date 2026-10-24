# Tests for ts_logger.record / read_key and ts_lead's join (no network).
import json, os, sys, tempfile, unittest
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ts_logger as L, ts_lead as J

SID = 2113944610368082408


class Logger(unittest.TestCase):
    def test_a_content_event_becomes_a_line_with_its_publish_time(self):
        ev = {"v": 1, "t": "tweet", "op": "content", "ts": 1, "d": {"tweetId": str(SID), "author": {"handle": "@DevvvOoooor"},
              "text": "SOL 2 is here", "kind": "post", "ref": {"type": "quote", "author": {"username": "x"}, "text": "q"}}}
        r = L.record(ev, 1_792_870_000_123.7)
        self.assertEqual((r["op"], r["id"], r["author"], r["rx"]), ("content", str(SID), "devvvooooor", 1_792_870_000_124))
        self.assertEqual(r["pub"], (SID >> 22) + L.TWITTER_EPOCH_MS)
        self.assertEqual(r["ref"], {"type": "quote", "author": "x", "text": "q"})
        # not a post, wrong version, no id
        self.assertIsNone(L.record({"v": 1, "t": "control", "op": "auth_pong", "d": {}}, 0))
        self.assertIsNone(L.record({"v": 2, "t": "tweet", "op": "content", "d": {"tweetId": "1"}}, 0))
        self.assertIsNone(L.record({"v": 1, "t": "tweet", "op": "content", "d": {}}, 0))

    def test_the_key_is_read_from_the_keys_file(self):
        with tempfile.NamedTemporaryFile("w", delete=False) as f:
            f.write('TRITON_URL = "https://x"\nTWEETSTREAM_KEY = "abc123"\n')
        os.environ.pop("TWEETSTREAM_API_KEY", None)
        self.assertEqual(L.read_key(f.name), "abc123")
        os.unlink(f.name)

    def test_a_second_accounts_key_is_read_by_its_name(self):
        with tempfile.NamedTemporaryFile("w", delete=False) as f:
            f.write('TWEETSTREAM_KEY2 = "second"\nTWEETSTREAM_KEY = "first"\n')
        os.environ.pop("TWEETSTREAM_API_KEY", None)
        self.assertEqual(L.read_key(f.name), "first")
        self.assertEqual(L.read_key(f.name, "TWEETSTREAM_KEY2"), "second")
        self.assertEqual(L.read_key(f.name, "TWEETSTREAM_KEY3"), "")
        os.unlink(f.name)


class Lead(unittest.TestCase):
    def test_a_post_before_the_create_has_a_positive_lead(self):
        with tempfile.TemporaryDirectory() as d:
            pub = (SID >> 22) + L.TWITTER_EPOCH_MS
            with open(d + "/t.jsonl", "w") as f:
                f.write(json.dumps({"rx": pub + 160, "op": "content", "id": str(SID), "pub": pub, "author": "devvvooooor", "text": "", "ref": None}) + "\n")
            seen1, seen2 = pub + 3_000, pub + 40_000   # the coins' creates, as the bot saw them
            with open(d + "/v.txt", "w") as f:
                f.write(f"{(seen1 + 600) / 1000:.6f} fra1 rugsnipe-hot[1]: tweet AbCdEf12 first rank 1 age 3s sid {SID} fetch 590ms verdict 600ms slot+2 ours\n")
                f.write(f"{(seen2 + 500) / 1000:.6f} fra1 rugsnipe-hot[1]: tweet ZzZzZz99 copy rank 2 age 40s sid {SID} fetch 480ms verdict 500ms slot+1 ours, cut to this slot\n")
                f.write("1792870000.000000 fra1 rugsnipe-hot[1]: operator x\n")
            on = J.report(J.load_posts(d + "/t.jsonl"), J.load_verdicts(d + "/v.txt"))
            self.assertEqual([round(x["lead"]) for x in on], [3_000 - 160, 40_000 - 160])
            self.assertEqual([x["rank"] for x in on], [1, 2])


if __name__ == "__main__":
    unittest.main()
