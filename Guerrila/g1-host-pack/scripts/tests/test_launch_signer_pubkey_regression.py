import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parents[1]
LANE_PATH = SCRIPT_DIR / "launch_follow_lane.py"


def load_lane_module():
    spec = importlib.util.spec_from_file_location("launch_follow_lane_test", LANE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    spec.loader.exec_module(module)
    return module


class LaunchSignerPubkeyRegressionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.lane = load_lane_module()

    def write_keypair(self, raw_values: list[int]) -> Path:
        handle = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8")
        with handle:
            json.dump(raw_values, handle)
        self.addCleanup(lambda: Path(handle.name).unlink(missing_ok=True))
        return Path(handle.name)

    def test_64_byte_keypair_uses_last_32_bytes_as_pubkey(self):
        raw_values = list(range(1, 65))
        keypair_path = self.write_keypair(raw_values)

        expected_pubkey = self.lane.b58encode(bytes(raw_values[-32:]))
        wrong_secret_half_pubkey = self.lane.b58encode(bytes(raw_values[:32]))

        self.assertNotEqual(expected_pubkey, wrong_secret_half_pubkey)
        self.assertEqual(self.lane.read_signer_pubkey(keypair_path), expected_pubkey)

    def test_invalid_short_keypair_raises(self):
        keypair_path = self.write_keypair([1, 2, 3])
        with self.assertRaises(RuntimeError):
            self.lane.read_signer_pubkey(keypair_path)


if __name__ == "__main__":
    unittest.main()
