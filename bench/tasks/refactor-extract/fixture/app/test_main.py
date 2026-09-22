import unittest
from main import run

class TestRun(unittest.TestCase):
    def test_run(self):
        self.assertEqual(run("hello world"), "Hello World [hello-world]")
