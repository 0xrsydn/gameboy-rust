"""Regressions for the reference scorer's SRAM log parsing; no shim or ROM needed."""
import unittest

import score_mgba_suite_reference as scorer


class FailuresTest(unittest.TestCase):
    def test_failures_carry_the_test_name_and_skip_the_header(self):
        log = (
            "mGBA suite log\n"
            "header: FAIL is not a result\n"
            "Timing test: ldr r2, [sp]\n"
            "Thumb/ROM P.S: Got     2 vs     3: FAIL\n"
            "Thumb/ROM PNS: Got     3 vs     3: PASS\n"
            "Hblank: Got 0x000004D0 vs 0x000004D3: FAIL  \n"
        )
        self.assertEqual(
            scorer.failures(log),
            [
                "ldr r2, [sp] | Thumb/ROM P.S: Got     2 vs     3: FAIL",
                "ldr r2, [sp] | Hblank: Got 0x000004D0 vs 0x000004D3: FAIL",
            ],
        )

    def test_failures_before_any_test_name_stay_bare(self):
        log = "a\nb\nM: SIOCNT: Got 0xEF8F vs 0x6F8F: FAIL\n"
        self.assertEqual(scorer.failures(log), ["M: SIOCNT: Got 0xEF8F vs 0x6F8F: FAIL"])


if __name__ == "__main__":
    unittest.main()
