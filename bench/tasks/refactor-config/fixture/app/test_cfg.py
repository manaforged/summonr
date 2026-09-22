import unittest
from service import whoami

class TestCfg(unittest.TestCase):
    def test_default(self):
        self.assertEqual(whoami(), "guest")
    def test_override(self):
        self.assertEqual(whoami({"user": "root"}), "root")
