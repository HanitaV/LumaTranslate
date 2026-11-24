from __future__ import annotations

import pathlib
import shutil
import subprocess


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
UNINSTALL_SCRIPT = REPO_ROOT / "installer" / "uninstall.ps1"


def _seed_fake_local_data(root: pathlib.Path) -> None:
    (root / "runtimes" / "ocr" / ".venv").mkdir(parents=True, exist_ok=True)
    (root / "runtimes" / "ocr" / ".venv" / ".luma-ready").write_text("ok", encoding="utf-8")
    (root / "models" / "paddleocr").mkdir(parents=True, exist_ok=True)
    (root / "models" / "paddleocr" / "weights.bin").write_bytes(b"1234")
    (root / "tools" / "uv").mkdir(parents=True, exist_ok=True)
    (root / "tools" / "uv" / "uv.exe").write_bytes(b"MZ")
    (root / "data").mkdir(parents=True, exist_ok=True)
    (root / "data" / "luma_translate.db").write_bytes(b"sqlite")
    (root / "ui-errors.log").write_text("err", encoding="utf-8")


def test_unattended_uninstall_removes_all_managed_data(tmp_path: pathlib.Path) -> None:
    fake_local = tmp_path / "LumaTranslateData"
    _seed_fake_local_data(fake_local)

    res = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(UNINSTALL_SCRIPT),
            "-Unattended",
            "-SkipRegistryAndShortcuts",
            "-LocalDataRoot",
            str(fake_local),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )

    assert res.returncode == 0, res.stderr
    assert not fake_local.exists()


def test_unattended_uninstall_can_keep_user_data(tmp_path: pathlib.Path) -> None:
    fake_local = tmp_path / "LumaTranslateKeepData"
    _seed_fake_local_data(fake_local)

    res = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(UNINSTALL_SCRIPT),
            "-Unattended",
            "-KeepUserData",
            "-SkipRegistryAndShortcuts",
            "-LocalDataRoot",
            str(fake_local),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )

    assert res.returncode == 0, res.stderr
    assert not (fake_local / "runtimes").exists()
    assert not (fake_local / "models").exists()
    assert not (fake_local / "tools").exists()
    assert (fake_local / "data" / "luma_translate.db").exists()


def test_unattended_uninstall_removes_standalone_app_folder_outside_git(tmp_path: pathlib.Path) -> None:
    standalone_dir = tmp_path / "StandaloneLumaApp"
    standalone_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(UNINSTALL_SCRIPT, standalone_dir / "uninstall.ps1")
    (standalone_dir / "LumaTranslate.exe").write_bytes(b"MZ")

    fake_local = tmp_path / "LumaTranslateStandaloneData"
    _seed_fake_local_data(fake_local)

    res = subprocess.run(
        [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(standalone_dir / "uninstall.ps1"),
            "-Unattended",
            "-RemoveAppFolder",
            "-SkipRegistryAndShortcuts",
            "-LocalDataRoot",
            str(fake_local),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )

    assert res.returncode == 0, res.stderr
    assert not fake_local.exists()

    # Wait briefly for the detached post-exit folder removal helper
    import time

    for _ in range(15):
        if not standalone_dir.exists():
            break
        time.sleep(0.3)

    assert not standalone_dir.exists()
