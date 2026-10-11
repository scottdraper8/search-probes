"""Exercise the real launcher with a disposable compiler/window fixture."""
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

LAUNCHER = Path(__file__).resolve().parents[1] / "preview-ui"


class WatcherTests(unittest.TestCase):
    def test_failed_edit_preserves_window_then_recovery_reloads_and_close_stops(self):
        with tempfile.TemporaryDirectory(prefix="search-probes-watcher-test-") as directory:
            root = Path(directory)
            (root / "tooling").mkdir()
            launcher = root / "tooling/preview-ui"
            shutil.copy2(LAUNCHER, launcher)
            processes = root / "processes"
            game = processes / "1000"
            game.mkdir(parents=True)
            game_name = game / "comm"
            game_name.write_text("NMS.exe\n")
            launcher.write_text(launcher.read_text().replace(
                'PROCESS_ROOT = Path("/proc")', f'PROCESS_ROOT = Path({str(processes)!r})'
            ))
            for crate in ("ui", "model"):
                source = root / f"crates/{crate}/src"
                source.mkdir(parents=True)
                (source.parent / "Cargo.toml").write_text("")
            (root / "Cargo.toml").write_text("")
            (root / "Cargo.lock").write_text("")
            source = root / "crates/ui/src/app.rs"
            source.write_text("initial")
            events = root / "windows.txt"
            user_close = root / "user-close"
            window_source = f'''#!{sys.executable}
from pathlib import Path
import os, sys, time
with Path({str(events)!r}).open('a') as stream:
 stream.write(str(os.getpid())+'\\n')
marker=Path(sys.argv[2])
while not marker.exists() and not Path({str(user_close)!r}).exists():
 time.sleep(.05)
'''
            compiler = root / "cargo-fixture"
            compiler.write_text(f'''#!{sys.executable}
from pathlib import Path
import os, sys
if 'FAIL' in Path({str(source)!r}).read_text():
 print('Fixture compilation failure', flush=True)
 sys.exit(1)
binary=Path(os.environ['CARGO_TARGET_DIR'])/'debug'/'search-probes-ui-preview'
binary.parent.mkdir(parents=True, exist_ok=True)
binary.write_text({window_source!r})
binary.chmod(0o755)
''')
            compiler.chmod(0o755)
            log = root / "watcher.log"
            environment = dict(os.environ, CARGO=str(compiler))
            environment.pop("CARGO_TARGET_DIR", None)
            with log.open("w") as output:
                watcher = subprocess.Popen(
                    [sys.executable, str(launcher)], cwd=root, env=environment,
                    stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                )
            try:
                def wait_for(condition):
                    deadline = time.monotonic() + 10
                    while time.monotonic() < deadline:
                        if condition():
                            return
                        time.sleep(.05)
                    self.fail(log.read_text())

                def windows():
                    return events.read_text().splitlines() if events.exists() else []

                wait_for(lambda: "UI preview suspended" in log.read_text())
                self.assertEqual(windows(), [])
                source.write_text("edit during gameplay")
                game_name.unlink()
                wait_for(lambda: len(windows()) == 1)
                first = int(windows()[0])
                source.write_text("FAIL")
                wait_for(lambda: "Build failed." in log.read_text())
                self.assertEqual(windows(), [str(first)])
                os.kill(first, 0)
                source.write_text("recovered")
                wait_for(lambda: len(windows()) == 2)
                self.assertNotEqual(windows()[1], str(first))
                self.assertIn("Build passed; reopening preview", log.read_text())
                game_name.write_text("NMS.exe\n")
                second = int(windows()[1])
                wait_for(lambda: not Path(f"/proc/{second}").exists())
                source.write_text("another gameplay edit")
                time.sleep(.8)
                self.assertEqual(len(windows()), 2)
                game_name.unlink()
                wait_for(lambda: len(windows()) == 3)
                user_close.touch()
                self.assertEqual(watcher.wait(timeout=5), 0)
            finally:
                if watcher.poll() is None:
                    watcher.send_signal(signal.SIGINT)
                    watcher.wait(timeout=10)


if __name__ == "__main__":
    unittest.main()
