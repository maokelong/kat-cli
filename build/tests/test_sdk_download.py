from __future__ import annotations

import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "build"))
import download_sdk_wheel
import payload_builder
from test_wheel_artifacts import write_sdk_wheel


class SdkDownloadTests(unittest.TestCase):
    def test_download_validates_release_bytes_and_reuses_offline_cache(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            wheel = root / "kat_sdk-0.1.0-py3-none-any.whl"
            write_sdk_wheel(wheel)
            wheel_bytes = wheel.read_bytes()
            (root / "build").mkdir()
            lock = {
                "version": "0.1.0",
                "artifact": {
                    "filename": wheel.name,
                    "url": f"https://example.com/v0.1.0/{wheel.name}",
                    "sha256": payload_builder.file_sha256(wheel),
                },
            }
            (root / "build/sdk-wheel.json").write_text(json.dumps(lock), encoding="utf-8")
            output = root / "download"
            with mock.patch(
                "payload_builder.urllib.request.urlopen", return_value=io.BytesIO(wheel_bytes)
            ) as download:
                result = download_sdk_wheel.download_sdk_wheel(root, output)
                self.assertEqual(result.read_bytes(), wheel_bytes)
                self.assertEqual(
                    download.call_args.args[0].full_url, lock["artifact"]["url"]
                )
            with mock.patch("payload_builder.urllib.request.urlopen") as download:
                self.assertEqual(
                    download_sdk_wheel.download_sdk_wheel(root, output, offline=True), result
                )
                download.assert_not_called()
            result.write_bytes(b"corrupted cached wheel")
            with self.assertRaisesRegex(ValueError, "SHA-256"):
                download_sdk_wheel.download_sdk_wheel(root, output, offline=True)

    def test_matching_digest_does_not_allow_wrong_sdk_version(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "build").mkdir()
            wheel = root / "kat_sdk-0.1.0-py3-none-any.whl"
            write_sdk_wheel(wheel)
            lock = {
                "version": "9.0.0",
                "artifact": {
                    "filename": wheel.name,
                    "url": f"https://example.com/{wheel.name}",
                    "sha256": payload_builder.file_sha256(wheel),
                },
            }
            (root / "build/sdk-wheel.json").write_text(json.dumps(lock), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "expected version"):
                download_sdk_wheel.download_sdk_wheel(root, root, offline=True)


if __name__ == "__main__":
    unittest.main()
