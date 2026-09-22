import sys
import unittest

from smoke_ongrow_console import smoke


class ConsoleSmokeTests(unittest.TestCase):
    def run_app(self, program):
        smoke([sys.executable, "-u", "-c", program], timeout=0.7, settle=0.1)

    def test_visible_ui_passes(self):
        self.run_app("import time; print('ONGROW_CONSOLE_UI_READY'); time.sleep(5)")

    def test_living_process_without_ui_fails(self):
        with self.assertRaisesRegex(RuntimeError, "rendered, visible"):
            self.run_app("import time; time.sleep(5)")

    def test_runtime_error_with_living_process_fails(self):
        with self.assertRaisesRegex(RuntimeError, "runtime error"):
            self.run_app("import time; print('LateInitializationError'); time.sleep(5)")

    def test_error_after_readiness_fails(self):
        with self.assertRaisesRegex(RuntimeError, "runtime error"):
            self.run_app("import time; print('ONGROW_CONSOLE_UI_READY'); print('EXCEPTION CAUGHT BY'); time.sleep(5)")

    def test_process_exit_fails(self):
        with self.assertRaisesRegex(RuntimeError, "exited"):
            self.run_app("pass")


if __name__ == "__main__":
    unittest.main()
